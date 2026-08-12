use proc_macro::TokenStream;

mod account_attr;
mod account_data_derive;
mod expr;
mod ty_map;

#[proc_macro_derive(AccountData)]
pub fn derive_account_data(input: TokenStream) -> TokenStream {
    account_data_derive::derive(input)
}

#[proc_macro_attribute]
pub fn account(args: TokenStream, input: TokenStream) -> TokenStream {
    account_attr::account(args, input)
}
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use sha2::{Digest, Sha256};
use syn::parse::{Parse, ParseStream};
use syn::{parse_macro_input, punctuated::Punctuated, Data, DeriveInput, Expr, Fields, Token};

/// One element of a `seeds = [...]` list.
#[derive(Clone)]
enum SeedElem {
    Literal(syn::LitByteStr), // b"vault"
    FieldKey(syn::Ident),     // field.key()
    /// `arg(off, len)` — DEPRECATED raw slice of `instr_data`. Kept working (removing it would
    /// break existing verified-anchor users), but no real Anchor program writes it.
    InstrArg(usize, usize),
    /// A named `#[instruction(...)]` argument — the form real Anchor uses. The surface form is
    /// carried along ONLY so the derive can check it against the argument's declared type; both
    /// forms resolve to the same bytes and the same Lean `SeedSpec.argField`.
    ArgField(syn::Ident, ArgSeedForm),
    /// A bare name reached by peeling `&` / `.as_ref()` / `.as_slice()` — e.g. `authority.as_ref()`
    /// or `&blob`. Whether it means an instruction argument or an account field is not decidable
    /// from syntax, so `derive_verified_accounts` rewrites it into `ArgField`/`FieldKey` before
    /// any codegen runs. It never survives to a `quote!`.
    Unresolved(syn::Ident),
}

/// How a `#[instruction(...)]` argument was spelled in a `seeds = [...]` list.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArgSeedForm {
    /// `name.as_bytes()` — Anchor's spelling for `String`/`Vec` arguments.
    AsBytes,
    /// `amount.to_le_bytes()` (optionally `.as_ref()`) — Anchor's spelling for numeric arguments.
    ToLeBytes,
    /// `authority.as_ref()` / `&blob` — Anchor's spelling for `Pubkey` and `Vec` arguments,
    /// which are already byte-shaped and need no conversion call.
    Bare,
}

impl ArgSeedForm {
    fn spelling(self) -> &'static str {
        match self {
            ArgSeedForm::AsBytes => "as_bytes()",
            ArgSeedForm::ToLeBytes => "to_le_bytes()",
            ArgSeedForm::Bare => "as_ref()",
        }
    }
}

/// Recognised field-type wrapper categories.
#[derive(Clone)]
#[allow(dead_code)]
enum WrapperKind {
    /// `Account<'info, T>` — type name is the inner T's ident.
    Account(syn::Ident),
    /// `Signer<'info>`.
    Signer,
    /// `Program<'info, P>` — full path of P (e.g. `verified_anchor::System`).
    Program(syn::Path),
    /// `SystemAccount<'info>`.
    SystemAccount,
    /// `UncheckedAccount<'info>` or `AccountInfo<'info>`.
    Unchecked,
}

/// Recognise a field's type as a wrapper. Returns an error for `u8` (bare u8 removed in M1b)
/// and an error span for unrecognised types.
fn classify_field_type(ty: &syn::Type) -> syn::Result<WrapperKind> {
    use syn::{PathArguments, Type, TypePath};
    if let Type::Path(TypePath { qself: None, path }) = ty {
        if path.is_ident("u8") {
            return Err(syn::Error::new_spanned(ty,
                "verified-anchor: bare `u8` field types are not supported; use a typed wrapper like `Account<'info, T>`, `Signer<'info>`, `UncheckedAccount<'info>`, etc. See docs/migrating-from-anchor.md"));
        }
        let last = path.segments.last().ok_or_else(|| {
            syn::Error::new_spanned(ty, "verified-anchor: unrecognised field type")
        })?;
        let ident_str = last.ident.to_string();
        match ident_str.as_str() {
            "Account" => {
                if let PathArguments::AngleBracketed(args) = &last.arguments {
                    for ga in &args.args {
                        if let syn::GenericArgument::Type(Type::Path(TypePath { qself: None, path: p })) = ga {
                            if let Some(seg) = p.segments.last() {
                                return Ok(WrapperKind::Account(seg.ident.clone()));
                            }
                        }
                    }
                }
                Err(syn::Error::new_spanned(ty, "Account<'info, T> requires a type argument"))
            }
            "Signer" => Ok(WrapperKind::Signer),
            "SystemAccount" => Ok(WrapperKind::SystemAccount),
            "UncheckedAccount" | "AccountInfo" => Ok(WrapperKind::Unchecked),
            "Program" => {
                if let PathArguments::AngleBracketed(args) = &last.arguments {
                    for ga in &args.args {
                        if let syn::GenericArgument::Type(Type::Path(TypePath { qself: None, path: p })) = ga {
                            // Keep the full path (e.g. `verified_anchor::System`) so code-gen
                            // can emit `<verified_anchor::System as ProgramId>::ID` etc.
                            return Ok(WrapperKind::Program(p.clone()));
                        }
                    }
                }
                Err(syn::Error::new_spanned(ty, "Program<'info, P> requires a type argument"))
            }
            _ => Err(syn::Error::new_spanned(ty,
                format!("verified-anchor: unrecognised field wrapper `{ident_str}`; use one of Account<'info, T>, Signer<'info>, Program<'info, P>, SystemAccount<'info>, UncheckedAccount<'info>, AccountInfo<'info>"))),
        }
    } else {
        Err(syn::Error::new_spanned(
            ty,
            "verified-anchor: unrecognised field type",
        ))
    }
}

/// The per-constraint implications of the field's wrapper kind.
/// `Account<T>` implies owner=crate::ID + discriminator=sha256("account:T")[..8].
/// `Signer` implies signer. `SystemAccount` implies owner=system_program::ID.
/// `Program<P>` synthesises a `ProgramMarker(P)` checked in validate_body.
/// `Unchecked` implies nothing.
fn wrapper_implied(kind: &WrapperKind) -> Vec<Constraint> {
    match kind {
        WrapperKind::Account(t) => {
            let mut h = Sha256::new();
            h.update(b"account:");
            h.update(t.to_string().as_bytes());
            let out = h.finalize();
            let mut d = [0u8; 8];
            d.copy_from_slice(&out[..8]);
            vec![
                Constraint::Owner(syn::parse_quote! { crate::ID }),
                Constraint::Discriminator(d),
            ]
        }
        WrapperKind::Signer => vec![Constraint::Signer],
        WrapperKind::SystemAccount => vec![Constraint::Owner(
            syn::parse_quote! { ::verified_anchor::solana_program::system_program::ID },
        )],
        WrapperKind::Program(p) => vec![Constraint::ProgramMarker(p.clone())],
        WrapperKind::Unchecked => vec![],
    }
}

/// One M2/M3 constraint parsed from a field's `#[account(...)]`.
#[derive(Clone)]
enum Constraint {
    Signer,
    Mut,
    Owner(Expr),
    HasOne(syn::Ident),
    /// Raw lifecycle markers — assembled into init/close steps, ignored by validate.
    InitMarker,
    Payer(syn::Ident),
    Space(usize),
    Close(syn::Ident),
    Seeds(Vec<SeedElem>),
    /// `seeds::program = <expr>` — derive the PDA against the FOREIGN program id `<expr>`
    /// instead of this program's id. Lean models it as the third `Constraint.seeds` field
    /// (`some Pubkey.zero` schematic placeholder; the soundness theorem is ∀ over the pubkey).
    SeedsProgram(Expr),
    BumpCanonical,
    BumpDeclared(u8),
    /// Opt-in, non-canonical "stored" bump: `bump = arg(off)`. The bump byte is read from the
    /// instruction data at byte offset `off`; the PDA is derived with THAT specific bump via
    /// `create_program_address` — NO canonical `find_program_address` requirement.
    BumpStored(usize),
    Discriminator([u8; 8]),
    /// `address = <expr>` — checks `accounts[i].key == expr`.
    Address(Expr),
    /// `executable` — checks `accounts[i].executable`.
    Executable,
    /// Implied by `Program<'info, P>` field type — checks executable + key == P::ID.
    /// Not parseable from `#[account(...)]`; emitted only by `wrapper_implied`.
    ProgramMarker(syn::Path),
    /// `allow_duplicate = <field>` — the per-pair opt-out for the struct-level distinct-mut-key
    /// check (M8.4). This field is explicitly permitted to alias `<field>`. A field may list
    /// several. Not a validation constraint; it tunes the struct-level pairwise check + emits
    /// the Lean `allowDuplicate` list.
    AllowDuplicate(syn::Ident),
    /// `rent_exempt = enforce` — the account must be rent-exempt at validation time.
    /// Emits `Constraint.rentExempt` in lean_spec and a `Rent::is_exempt` runtime check.
    RentExemptEnforce,
    /// `rent_exempt = skip` — explicitly opt out of the rent-exemption check.
    /// Emits nothing in lean_spec and no runtime check (SAFE-BY-DEFAULT opt-out).
    RentExemptSkip,
    /// `realloc = <newLen>` — resize the account data to `<newLen>` total bytes (lifecycle).
    Realloc(usize),
    /// `realloc::payer = <ident>` — funds the rent top-up on grow.
    ReallocPayer(syn::Ident),
    /// `realloc::zero = <bool>` — zero-fill the grown region.
    ReallocZero(bool),
    /// `zero` — the all-zero-discriminator reinit guard (a VALIDATION constraint).
    Zero,
    /// `init_if_needed` — conditional init (lifecycle); reuses `Payer`/`Space`.
    InitIfNeeded,
    /// `constraint = <expr>` — the RAW parsed expression. Compilation into the proven
    /// relational sublanguage (`expr::compile_expr`) is deferred until field indices are known,
    /// which is only true once every field has been collected.
    Expr(Expr),
}

impl Parse for Constraint {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // `mut` is a keyword — must peek the token, not parse an Ident.
        if input.peek(Token![mut]) {
            input.parse::<Token![mut]>()?;
            return Ok(Constraint::Mut);
        }
        let ident: syn::Ident = input.parse()?;
        match ident.to_string().as_str() {
            "signer" => Ok(Constraint::Signer),
            "executable" => Ok(Constraint::Executable),
            "owner" => {
                input.parse::<Token![=]>()?;
                let expr: Expr = input.parse()?;
                Ok(Constraint::Owner(expr))
            }
            "address" => {
                input.parse::<Token![=]>()?;
                let expr: Expr = input.parse()?;
                Ok(Constraint::Address(expr))
            }
            "has_one" => {
                input.parse::<Token![=]>()?;
                let target: syn::Ident = input.parse()?;
                Ok(Constraint::HasOne(target))
            }
            "allow_duplicate" => {
                input.parse::<Token![=]>()?;
                let target: syn::Ident = input.parse()?;
                Ok(Constraint::AllowDuplicate(target))
            }
            "init" => Ok(Constraint::InitMarker),
            "payer" => {
                input.parse::<Token![=]>()?;
                Ok(Constraint::Payer(input.parse()?))
            }
            "space" => {
                input.parse::<Token![=]>()?;
                let lit: syn::LitInt = input.parse()?;
                Ok(Constraint::Space(lit.base10_parse()?))
            }
            "close" => {
                input.parse::<Token![=]>()?;
                Ok(Constraint::Close(input.parse()?))
            }
            "seeds" => {
                // `seeds::program = <expr>` — the `::`-path key for a foreign-program PDA
                // derivation. Parsed alongside `seeds = [..]` / `bump` on the same field.
                if input.peek(Token![::]) {
                    input.parse::<Token![::]>()?;
                    let key: syn::Ident = input.parse()?;
                    if key != "program" {
                        return Err(syn::Error::new(
                            key.span(),
                            "unsupported `seeds::` key (expected `seeds::program = <expr>`)",
                        ));
                    }
                    input.parse::<Token![=]>()?;
                    let expr: Expr = input.parse()?;
                    return Ok(Constraint::SeedsProgram(expr));
                }
                input.parse::<Token![=]>()?;
                let arr: syn::ExprArray = input.parse()?;
                let mut elems = Vec::new();
                for e in arr.elems {
                    elems.push(parse_seed_elem(e)?);
                }
                Ok(Constraint::Seeds(elems))
            }
            "bump" => {
                if input.peek(Token![=]) {
                    input.parse::<Token![=]>()?;
                    // `bump = <litint>` (declared) vs `bump = arg(off)` (stored, non-canonical).
                    let expr: Expr = input.parse()?;
                    match expr {
                        Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Int(i),
                            ..
                        }) => Ok(Constraint::BumpDeclared(i.base10_parse()?)),
                        Expr::Call(call) => {
                            let is_arg = matches!(call.func.as_ref(),
                                Expr::Path(p) if p.path.is_ident("arg"));
                            if !is_arg {
                                return Err(syn::Error::new_spanned(call.func,
                                    "unsupported `bump = <expr>` (expected a u8 literal or `arg(off)`)"));
                            }
                            let off = lit_usize(call.args.iter().next())?;
                            Ok(Constraint::BumpStored(off))
                        }
                        other => Err(syn::Error::new_spanned(
                            other,
                            "unsupported `bump = <expr>` (expected a u8 literal or `arg(off)`)",
                        )),
                    }
                } else {
                    Ok(Constraint::BumpCanonical)
                }
            }
            "discriminator" => {
                input.parse::<Token![=]>()?;
                let lit: syn::LitStr = input.parse()?;
                let mut h = Sha256::new();
                h.update(b"account:");
                h.update(lit.value().as_bytes());
                let out = h.finalize();
                let mut d = [0u8; 8];
                d.copy_from_slice(&out[..8]);
                Ok(Constraint::Discriminator(d))
            }
            "rent_exempt" => {
                input.parse::<Token![=]>()?;
                let mode: syn::Ident = input.parse()?;
                match mode.to_string().as_str() {
                    "enforce" => Ok(Constraint::RentExemptEnforce),
                    "skip" => Ok(Constraint::RentExemptSkip),
                    other => Err(syn::Error::new(
                        mode.span(),
                        format!(
                            "expected `enforce` or `skip` after `rent_exempt =`, got `{other}`"
                        ),
                    )),
                }
            }
            "zero" => Ok(Constraint::Zero),
            "init_if_needed" => Ok(Constraint::InitIfNeeded),
            // Deliberately parsed as an arbitrary `syn::Expr` and NOT validated here: whether
            // it lands inside the proven sublanguage is decided later, by `expr::compile_expr`,
            // and an expression that does not is routed to the escape hatch rather than
            // rejected. Real Anchor accepts arbitrary Rust here, so refusing to parse would
            // break the drop-in property outright.
            "constraint" => {
                input.parse::<Token![=]>()?;
                Ok(Constraint::Expr(input.parse()?))
            }
            "realloc" => {
                // `realloc::payer = <ident>` / `realloc::zero = <bool>` / `realloc = <n>`
                // Mirror the `seeds`/`seeds::program` `::`-peek pattern.
                if input.peek(Token![::]) {
                    input.parse::<Token![::]>()?;
                    let key: syn::Ident = input.parse()?;
                    input.parse::<Token![=]>()?;
                    match key.to_string().as_str() {
                        "payer" => Ok(Constraint::ReallocPayer(input.parse()?)),
                        "zero" => {
                            let b: syn::LitBool = input.parse()?;
                            Ok(Constraint::ReallocZero(b.value))
                        }
                        other => Err(syn::Error::new(key.span(),
                            format!("unsupported `realloc::` key `{other}` (expected `payer` or `zero`)"))),
                    }
                } else {
                    input.parse::<Token![=]>()?;
                    let lit: syn::LitInt = input.parse()?;
                    Ok(Constraint::Realloc(lit.base10_parse()?))
                }
            }
            other => {
                let known_unsupported = [
                    "token",
                    "mint",
                    "associated_token",
                    "owner_program",
                    "token_program",
                ];
                let hint = if known_unsupported.contains(&other) {
                    format!("`{other}` is a stock-Anchor constraint that verified-anchor does not support")
                } else {
                    format!("unknown constraint `{other}`")
                };
                Err(syn::Error::new(
                    ident.span(),
                    format!("{hint}; verified-anchor supports: signer, mut, owner, has_one, allow_duplicate, init, init_if_needed, payer, space, close, seeds, seeds::program, bump, discriminator, address, executable, rent_exempt, realloc, realloc::payer, realloc::zero, zero, constraint. See docs/migrating-from-anchor.md"),
                ))
            }
        }
    }
}

/// Peel the wrappers Anchor seed lists put around a seed source, down to the name and the
/// surface form used.
///
/// `&`-refs and trailing `.as_ref()`/`.as_slice()` are peeled because a seed list is `&[&[u8]]`:
/// real Anchor source writes `user.key().as_ref()`, `amount.to_le_bytes().as_ref()`,
/// `&amount.to_le_bytes()` and `&blob`, because the bare values (`Pubkey`, `[u8; 8]`, `Vec<u8>`)
/// do not coerce to `&[u8]` in that position. That wrapping carries no meaning for us — the
/// account, or the argument's declared type, is what determines the bytes.
fn peel_seed(e: &Expr) -> Option<SeedElem> {
    match e {
        Expr::Reference(r) => peel_seed(&r.expr),
        // Reached only by peeling (a NAKED path is not accepted as a seed): `&blob`,
        // `blob.as_slice()`, `authority.as_ref()`. Which binding it names is resolved later.
        Expr::Path(p) => p
            .path
            .get_ident()
            .map(|id| SeedElem::Unresolved(id.clone())),
        // The peeling applies to LITERAL seeds too: `b"vault".as_ref()` and `&b"vault"` are the
        // same seed as the bare `b"vault"` that `parse_seed_elem` matches first.
        Expr::Lit(syn::ExprLit {
            lit: syn::Lit::ByteStr(b),
            ..
        }) => Some(SeedElem::Literal(b.clone())),
        Expr::MethodCall(mc) if mc.args.is_empty() => {
            let recv_ident = || match mc.receiver.as_ref() {
                Expr::Path(p) => p.path.get_ident().cloned(),
                _ => None,
            };
            match mc.method.to_string().as_str() {
                // `.key()` is unambiguous: only an account has one. No instruction-argument type
                // verified-anchor can map exposes `.key()`, so this never needs resolution.
                "key" => recv_ident().map(SeedElem::FieldKey),
                // `.as_bytes()`/`.to_le_bytes()` are equally unambiguous the other way: no
                // account wrapper exposes either.
                "as_bytes" => match mc.receiver.as_ref() {
                    // `"vault".as_bytes()` — the str-literal spelling of a literal seed. Anchor
                    // programs use it interchangeably with `b"vault"`; same bytes, so same seed.
                    Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(sl),
                        ..
                    }) => Some(SeedElem::Literal(syn::LitByteStr::new(
                        sl.value().as_bytes(),
                        sl.span(),
                    ))),
                    _ => recv_ident().map(|id| SeedElem::ArgField(id, ArgSeedForm::AsBytes)),
                },
                "to_le_bytes" => {
                    recv_ident().map(|id| SeedElem::ArgField(id, ArgSeedForm::ToLeBytes))
                }
                // Slice-coercion noise, not a seed source: keep peeling.
                "as_ref" | "as_slice" => peel_seed(&mc.receiver),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Find a `to_be_bytes()`/`to_ne_bytes()` anywhere in a seed expression, for the endianness
/// guard. Native-endian is refused alongside big-endian: it happens to be little-endian on BPF,
/// so it would work by accident and break the moment anything evaluates the spec off-chain.
fn big_or_native_endian_seed(e: &Expr) -> Option<String> {
    match e {
        Expr::Reference(r) => big_or_native_endian_seed(&r.expr),
        Expr::MethodCall(mc) => {
            let m = mc.method.to_string();
            if m == "to_be_bytes" || m == "to_ne_bytes" {
                return Some(format!("{m}()"));
            }
            big_or_native_endian_seed(&mc.receiver)
        }
        _ => None,
    }
}

fn parse_seed_elem(e: Expr) -> syn::Result<SeedElem> {
    match e {
        Expr::Lit(syn::ExprLit {
            lit: syn::Lit::ByteStr(b),
            ..
        }) => Ok(SeedElem::Literal(b)),
        // `user.key()` / `user.key().as_ref()` / `name.as_bytes()` / `amount.to_le_bytes()`.
        // These are the forms real Anchor source writes, and the whole point of M10 Task 9: an
        // unmodified Anchor `#[derive(Accounts)]` struct must compile under
        // `#[derive(VerifiedAccounts)]`.
        Expr::MethodCall(_) | Expr::Reference(_) => {
            // WRONG-ENDIANNESS GUARD, checked before anything else. `to_be_bytes()` would derive
            // a DIFFERENT address than the same source under real Anchor — silently, since a PDA
            // mismatch looks like "wrong account" rather than "wrong seed encoding". Borsh (and
            // therefore Lean `argBytes`' fixed-size arm) is little-endian, so there is no correct
            // way to honour it: refuse to compile rather than derive a wrong address.
            if let Some(bad) = big_or_native_endian_seed(&e) {
                return Err(syn::Error::new_spanned(
                    &e,
                    format!(
                    "seed `{bad}` is not supported: Borsh — and therefore the PDA Anchor derives \
                     — is LITTLE-endian, so this would silently derive a different address than \
                     the same program under Anchor. Use `to_le_bytes()`."),
                ));
            }
            match peel_seed(&e) {
                Some(se) => Ok(se),
                None => Err(syn::Error::new_spanned(
                    &e,
                    "unsupported seed (expected b\"..\", field.key(), name.as_bytes(), \
                     amount.to_le_bytes(), a Pubkey/Vec argument via `.as_ref()`, \
                     or arg(off, len))",
                )),
            }
        }
        Expr::Call(call) => {
            let is_arg = matches!(call.func.as_ref(),
                Expr::Path(p) if p.path.is_ident("arg"));
            if !is_arg {
                return Err(syn::Error::new_spanned(
                    call.func,
                    "unsupported seed call (expected `arg(off, len)`)",
                ));
            }
            let mut it = call.args.iter();
            let off = lit_usize(it.next())?;
            let len = lit_usize(it.next())?;
            Ok(SeedElem::InstrArg(off, len))
        }
        other => Err(syn::Error::new_spanned(
            other,
            "unsupported seed (expected b\"..\", field.key(), name.as_bytes(), or arg(off, len))",
        )),
    }
}

fn lit_usize(e: Option<&Expr>) -> syn::Result<usize> {
    match e {
        Some(Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(i), .. })) => i.base10_parse(),
        _ => Err(syn::Error::new(proc_macro2::Span::call_site(),
            "expected an integer literal (seeds `arg` needs two: arg(off, len); bump `arg` needs one: arg(off))")),
    }
}

/// One declared `#[instruction(...)]` argument.
struct InstrArg {
    name: String,
    /// Runtime `Ty` expression (the `verified_anchor::layout::Ty` this argument decodes as).
    rt: TokenStream2,
    /// The same descriptor as Lean `Ty` source, for the emitted `instrArgs` list.
    lean: String,
    /// Which seed spelling this argument accepts. Purely a surface check — it does not affect
    /// the bytes, which come from `rt` alone.
    kind: ArgTyKind,
}

/// Coarse type category of a `#[instruction(...)]` argument, used to check that a seed is
/// spelled the way Anchor would spell it for that type.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArgTyKind {
    /// `String`/`Vec<_>` — Borsh length-prefixed; Anchor writes `.as_bytes()`.
    Prefixed,
    /// Fixed-width integers — Anchor writes `.to_le_bytes()`.
    Numeric,
    /// `Pubkey` — already byte-shaped; Anchor writes `.as_ref()`.
    Key,
    /// Everything else we can map (`bool`, `Option<_>`): no seed spelling supported yet.
    Other,
}

/// Classify by the type's OUTERMOST path segment, which is what decides the Borsh framing:
/// `Option<String>` is length-prefixed at the `Option` layer, not the `String` layer, so it is
/// `Other` rather than `Prefixed`.
fn classify_arg_ty(ty: &syn::Type) -> ArgTyKind {
    let seg = match ty {
        syn::Type::Path(p) => match p.path.segments.last() {
            Some(seg) => seg,
            None => return ArgTyKind::Other,
        },
        _ => return ArgTyKind::Other,
    };
    let name = seg.ident.to_string();
    match name.as_str() {
        "String" => ArgTyKind::Prefixed,
        // ONLY `Vec<u8>` is byte-shaped. For `Vec<T>` with a wider `T` the 4-byte prefix counts
        // ELEMENTS, not bytes, so a seed would silently take `count` bytes instead of
        // `count * size_of::<T>()`. Anchor could not compile such a seed either (`&Vec<u32>` is
        // not `&[u8]`), so refusing it costs nothing and removes a way to get wrong seed bytes.
        "Vec" => match vec_elem_is_u8(seg) {
            true => ArgTyKind::Prefixed,
            false => ArgTyKind::Other,
        },
        "u8" | "u16" | "u32" | "u64" | "u128" | "i8" | "i16" | "i32" | "i64" | "i128" => {
            ArgTyKind::Numeric
        }
        "Pubkey" => ArgTyKind::Key,
        _ => ArgTyKind::Other,
    }
}

/// Every `#[instruction(...)]` argument the developer WROTE, plus the prefix of them whose types
/// verified-anchor can locate.
struct InstrArgs {
    /// The mappable prefix — the only ones whose Borsh offset is computable, so the only ones a
    /// seed may resolve against.
    mappable: Vec<InstrArg>,
    /// EVERY declared name, including those at and after the unmappable cutoff.
    ///
    /// Load-bearing for correctness, not diagnostics: without it, a dropped argument whose name
    /// also matches an account field would resolve to that FIELD, silently deriving a different
    /// address than Anchor (which evaluates the seed with the argument in scope). Keeping the raw
    /// list lets the resolution pass tell "you never declared this" apart from "you declared it
    /// but we dropped it", and refuse both instead of falling through.
    declared: Vec<String>,
    /// Every declared argument WITH its Rust type, in declaration order.
    ///
    /// The escape hatch (M10 Task 13) decodes arguments with Borsh SEQUENTIALLY from the front
    /// of `instr_data`, so it needs the declared type of every argument up to the one it wants
    /// — including the ones `mappable` had to drop, whose Borsh offset is uncomputable from a
    /// descriptor but perfectly computable by decoding what precedes them.
    all: Vec<(String, syn::Type)>,
}

/// Is this `Vec<...>` segment's element type exactly `u8`?
fn vec_elem_is_u8(seg: &syn::PathSegment) -> bool {
    let syn::PathArguments::AngleBracketed(a) = &seg.arguments else {
        return false;
    };
    a.args.iter().any(|g| {
        matches!(g,
        syn::GenericArgument::Type(syn::Type::Path(p)) if p.path.is_ident("u8"))
    })
}

/// Parse `#[instruction(amount: u64, name: String)]` off the derive input.
///
/// An argument whose type is not mappable STOPS the mappable list: every argument after it sits
/// at an offset we cannot compute (Borsh is positional and variable-width), so silently keeping
/// it would hand later arguments a wrong offset. This is the same cutoff rule the account-layout
/// derive uses. The names are still recorded in `declared`, so a seed naming one is a compile
/// error rather than a runtime brick OR a silent fallback to a same-named account.
fn parse_instruction_args(input: &DeriveInput) -> syn::Result<InstrArgs> {
    let attr = match input
        .attrs
        .iter()
        .find(|a| a.path().is_ident("instruction"))
    {
        Some(a) => a,
        None => {
            return Ok(InstrArgs {
                mappable: Vec::new(),
                declared: Vec::new(),
                all: Vec::new(),
            })
        }
    };
    let parser = Punctuated::<syn::PatType, Token![,]>::parse_terminated;
    let parsed = attr.parse_args_with(parser)?;
    let mut mappable = Vec::new();
    let mut declared = Vec::new();
    let mut all = Vec::new();
    let mut past_cutoff = false;
    for pt in parsed {
        let name = match pt.pat.as_ref() {
            syn::Pat::Ident(i) => i.ident.to_string(),
            other => return Err(syn::Error::new_spanned(other, "expected an argument name")),
        };
        declared.push(name.clone());
        all.push((name.clone(), (*pt.ty).clone()));
        // Keep walking after the cutoff purely to finish recording names — nothing past it can
        // be mapped, because its offset depends on the width we could not compute.
        if past_cutoff {
            continue;
        }
        match crate::ty_map::map_ty(&pt.ty) {
            Some((rt, lean)) => mappable.push(InstrArg {
                name,
                rt,
                lean,
                kind: classify_arg_ty(&pt.ty),
            }),
            None => past_cutoff = true,
        }
    }
    Ok(InstrArgs {
        mappable,
        declared,
        all,
    })
}

/// Borsh bindings for the `#[instruction(...)]` arguments an escape-hatch expression names.
///
/// Emitted INSIDE the check's own block, never at the top of `try_accounts`: decoding is
/// fallible, and an argument needed only by a check that a const-selected branch switched off
/// must not be decoded (let alone fail) in builds where that branch is dead.
fn instr_arg_binds(
    all: &[(String, syn::Type)],
    used: &std::collections::HashSet<String>,
    field_names: &[String],
    field: &str,
    src: &str,
    span: proc_macro2::Span,
) -> TokenStream2 {
    // A name that is both an argument and an account field resolves to the field, which is
    // already bound; decoding it here would shadow the field the developer meant.
    let wanted = |n: &String| used.contains(n) && !field_names.contains(n);
    let Some(last) = all.iter().rposition(|(n, _)| wanted(n)) else {
        return quote! {};
    };
    let decodes: Vec<TokenStream2> = all[..=last]
        .iter()
        .map(|(n, ty)| {
            // Arguments before the last referenced one are decoded only to advance the cursor.
            let pat = match wanted(n) {
                true => {
                    let id = syn::Ident::new(n, span);
                    quote! { #id }
                }
                false => quote! { _ },
            };
            quote! {
                // BELT AND BRACES with `idents_in`'s value-position filter. That filter removes the
                // common false positive, but the walk stays an over-approximation by design, and any
                // residual one lands here as an `unused_variables` warning attributed to the USER'S
                // STRUCT SPAN — unsilenceable from their crate and a hard error under
                // `#![deny(warnings)]`, which is the `compile_error!` the prime directive forbids.
                #[allow(unused_variables)]
                let #pat: #ty = <#ty as ::verified_anchor::borsh::BorshDeserialize>::deserialize(
                    &mut __va_args)
                    .map_err(|_| ::verified_anchor::VAError::ConstraintViolated {
                        field: #field, expr: #src })?;
            }
        })
        .collect();
    quote! {
        let mut __va_args: &[u8] = instr_data;
        #(#decodes)*
    }
}

/// Does any field resolve a `name.as_bytes()` seed? Gates emission of the `INSTR_ARGS` const.
fn uses_arg_field(specs: &[FieldSpec]) -> bool {
    specs.iter().any(|s| {
        s.constraints.iter().any(|c| {
            matches!(c,
        Constraint::Seeds(elems) if elems.iter().any(|e| matches!(e, SeedElem::ArgField(_, _))))
        })
    })
}

/// The `INSTR_ARGS` Borsh field list, emitted as a local `const` in any generated function that
/// resolves a `name.as_bytes()` seed. Mirrors Lean's `Ty.struct s.instrArgs`, which `argBytes`
/// walks with `locate`.
fn instr_args_const(instr_args: &[InstrArg]) -> TokenStream2 {
    let names = instr_args.iter().map(|a| a.name.as_str());
    let tys = instr_args.iter().map(|a| &a.rt);
    quote! {
        const INSTR_ARGS: &[(&str, ::verified_anchor::layout::Ty)] = &[#((#names, #tys)),*];
    }
}

/// Runtime bytes of the named `#[instruction(...)]` argument `arg`, as a seed uses them.
///
/// THIS MUST MIRROR Lean `AccountsStruct.argBytes` (Constraints/Context.lean) ARM FOR ARM —
/// a divergence here makes verified-anchor derive a DIFFERENT PDA than real Anchor, which our
/// own tests cannot catch (they would agree with our own model) and which surfaces only as a
/// production address mismatch. The two arms are:
///   * `string`/`vec` — read the 4-byte LE length prefix and return the PAYLOAD ONLY. Anchor's
///     `name.as_bytes()` yields the string's bytes, NOT the Borsh framing, so the length prefix
///     is stripped. Bounds-checked as Lean's `off + 4 + n ≤ size`.
///   * everything else — return the whole encoding, width from `encodedWidth`, bounds-checked
///     as Lean's `off + w ≤ size`.
/// Both arms fail closed; here "closed" is `WrongPda` on `field`, matching Lean's `none`
/// (an unresolvable seed cannot produce a matching PDA).
///
/// `instr_data` is the argument buffer with any instruction discriminator ALREADY STRIPPED by
/// the caller — exactly what Anchor hands `try_accounts` — so decoding starts at offset 0.
fn arg_field_seed_expr(arg: &syn::Ident, fname: &str) -> TokenStream2 {
    let n = arg.to_string();
    quote! {
        {
            let __ty = ::verified_anchor::layout::Ty::Struct(INSTR_ARGS);
            match ::verified_anchor::layout::locate(&__ty, &[#n], instr_data, 0) {
                ::core::option::Option::Some((__off, ::verified_anchor::layout::Ty::String))
                | ::core::option::Option::Some((__off, ::verified_anchor::layout::Ty::Vec(_))) => {
                    let __len = u32::from_le_bytes(
                        instr_data.get(__off..__off.wrapping_add(4))
                            .and_then(|s| <[u8; 4]>::try_from(s).ok())
                            .ok_or(::verified_anchor::VAError::WrongPda { field: #fname })?
                    ) as usize;
                    let __start = __off.wrapping_add(4);
                    let __end = match __start.checked_add(__len) {
                        ::core::option::Option::Some(e) => e,
                        ::core::option::Option::None =>
                            return Err(::verified_anchor::VAError::WrongPda { field: #fname }),
                    };
                    instr_data.get(__start..__end)
                        .ok_or(::verified_anchor::VAError::WrongPda { field: #fname })?
                }
                ::core::option::Option::Some((__off, __fty)) => {
                    let __w = ::verified_anchor::layout::encoded_width(&__fty, instr_data, __off)
                        .ok_or(::verified_anchor::VAError::WrongPda { field: #fname })?;
                    let __end = match __off.checked_add(__w) {
                        ::core::option::Option::Some(e) => e,
                        ::core::option::Option::None =>
                            return Err(::verified_anchor::VAError::WrongPda { field: #fname }),
                    };
                    instr_data.get(__off..__end)
                        .ok_or(::verified_anchor::VAError::WrongPda { field: #fname })?
                }
                ::core::option::Option::None =>
                    return Err(::verified_anchor::VAError::WrongPda { field: #fname }),
            }
        }
    }
}

struct FieldSpec {
    name: String,
    constraints: Vec<Constraint>,
    kind: WrapperKind,
}

fn collect_fields(input: &DeriveInput) -> syn::Result<Vec<FieldSpec>> {
    let Data::Struct(ds) = &input.data else {
        return Err(syn::Error::new_spanned(
            input,
            "VerifiedAccounts requires a struct",
        ));
    };
    let Fields::Named(named) = &ds.fields else {
        return Err(syn::Error::new_spanned(
            &ds.fields,
            "VerifiedAccounts requires named fields",
        ));
    };
    let mut specs = Vec::new();
    for field in &named.named {
        let name = field.ident.as_ref().unwrap().to_string();
        let mut constraints = Vec::new();
        for attr in &field.attrs {
            if attr.path().is_ident("account") {
                let parsed =
                    attr.parse_args_with(Punctuated::<Constraint, Token![,]>::parse_terminated)?;
                constraints.extend(parsed);
            }
        }
        let kind = classify_field_type(&field.ty)?;
        specs.push(FieldSpec {
            name,
            constraints,
            kind,
        });
    }
    Ok(specs)
}

/// The name→index and name→inner-type maps `expr::ExprCtx` needs.
///
/// Built by one function used from BOTH `validate_body` and `lean_spec_string`: the runtime
/// check and the Lean spec must agree on how every name in a `constraint = <expr>` resolves, and
/// two independent copies of this resolution could drift into emitting a check for one account
/// while the spec names another.
fn expr_maps(
    specs: &[FieldSpec],
) -> (
    std::collections::HashMap<String, usize>,
    std::collections::HashMap<String, syn::Type>,
) {
    let index_of = specs
        .iter()
        .enumerate()
        .map(|(i, s)| (s.name.clone(), i))
        .collect();
    // Only `Account<'info, T>` has a modelled Borsh layout, so only those fields can carry a
    // data-field operand. Everything else resolves to metadata operands or falls out.
    let inner_ty = specs
        .iter()
        .filter_map(|s| match &s.kind {
            WrapperKind::Account(t) => {
                let ty: syn::Type = syn::parse_quote! { #t };
                Some((s.name.clone(), ty))
            }
            _ => None,
        })
        .collect();
    (index_of, inner_ty)
}

/// A readable rendering of a `constraint = <expr>`, for the `ConstraintViolated` error and for
/// `UNPROVEN_CHECKS`. Purely diagnostic — nothing branches on it.
///
/// Deliberately NOT `Span::source_text()`: on stable, `Spanned::span()` for a multi-token
/// expression cannot join spans and falls back to the FIRST token, so `vault.amount >= 1000`
/// came back as just `"vault"`. Rendering the tokens is the only faithful option.
/// `tests/behavior.rs::constraint_violation_names_the_expression` pins the result.
fn expr_source(e: &Expr) -> String {
    let mut out = String::new();
    let mut st = RenderState {
        prev: Prev::Start,
        prev_joint: false,
        run_role: Prev::Start,
    };
    render_tokens(quote!(#e), &mut out, &mut st);
    out
}

/// What the previously emitted token was, which is all the spacing rules need to know.
#[derive(Clone, Copy, PartialEq)]
enum Prev {
    /// Nothing yet, or just after an opening delimiter.
    Start,
    /// An identifier or a literal.
    Word,
    /// A closing delimiter (or a postfix `?`): an operand ENDED here.
    Close,
    /// A punct in binary/separator position — `>=`, `,`. A space follows it.
    Op,
    /// A punct in prefix position (`&x`, `-1`) or a glue punct (`.`, `::`, a macro `!`).
    /// No space follows it.
    Prefix,
}

struct RenderState {
    prev: Prev,
    /// The previous punct was `Spacing::Joint`, i.e. the next punct continues the SAME
    /// operator (`>` `=` -> `>=`). Multi-char operators must not be split by a space.
    prev_joint: bool,
    /// The role assigned to the first punct of the current joint run, inherited by the rest.
    run_role: Prev,
}

/// Render a token stream the way a human would have written it.
///
/// `quote!` puts a space around EVERY token, which turns `is_blessed(a.key)` into
/// `is_blessed (a . key)` — unreadable in an error message and in the `UNPROVEN_CHECKS`
/// report, where arbitrary developer Rust now shows up (M10 Task 13). The two string
/// substitutions this replaced only covered field access and empty argument lists.
///
/// The one genuinely ambiguous case is prefix vs binary `&`/`*`/`-`: they are told apart by
/// what precedes them, exactly as a parser does — an operator with nothing to its left to
/// consume (start of stream, or right after another operator) is a prefix operator, so
/// `a.owner == &crate::ID` renders with the `&` glued to `crate`.
fn render_tokens(ts: TokenStream2, out: &mut String, st: &mut RenderState) {
    use proc_macro2::{Delimiter, Spacing, TokenTree};
    for tt in ts {
        match tt {
            TokenTree::Ident(_) | TokenTree::Literal(_) => {
                if matches!(st.prev, Prev::Word | Prev::Close | Prev::Op) {
                    out.push(' ');
                }
                out.push_str(&tt.to_string());
                st.prev = Prev::Word;
                st.prev_joint = false;
            }
            TokenTree::Group(g) => {
                let (open, close) = match g.delimiter() {
                    Delimiter::Parenthesis => ("(", ")"),
                    Delimiter::Bracket => ("[", "]"),
                    Delimiter::Brace => ("{ ", " }"),
                    // An invisible group is exactly what a macro-substituted expression looks
                    // like after token capture; it must render as nothing at all.
                    Delimiter::None => ("", ""),
                };
                // A call/index binds tight (`f(x)`, `v[0]`); a grouping paren after an
                // operator does not (`a && (b || c)`).
                let is_call = matches!(st.prev, Prev::Word | Prev::Close)
                    && !matches!(g.delimiter(), Delimiter::Brace | Delimiter::None);
                if !is_call && matches!(st.prev, Prev::Word | Prev::Close | Prev::Op) {
                    out.push(' ');
                }
                out.push_str(open);
                st.prev = Prev::Start;
                st.prev_joint = false;
                render_tokens(g.stream(), out, st);
                out.push_str(close);
                st.prev = Prev::Close;
                st.prev_joint = false;
            }
            TokenTree::Punct(p) => {
                let ch = p.as_char();
                let alone = p.spacing() == Spacing::Alone;
                // A macro bang: `matches!(..)`. `!=` is a JOINT `!`, so the two cannot collide.
                let macro_bang = ch == '!' && alone && st.prev == Prev::Word;
                let space = if st.prev_joint {
                    false
                } else if macro_bang {
                    false
                } else {
                    match ch {
                        '.' | ',' | ';' | ':' | '#' | '?' => false,
                        _ => matches!(st.prev, Prev::Word | Prev::Close | Prev::Op),
                    }
                };
                if space {
                    out.push(' ');
                }
                out.push(ch);
                let role = if st.prev_joint {
                    st.run_role
                } else if macro_bang {
                    Prev::Prefix
                } else {
                    match ch {
                        ',' | ';' => Prev::Op,
                        '.' | ':' | '#' => Prev::Prefix,
                        // Postfix `?`: an operand ended here, like a closing delimiter.
                        '?' => Prev::Close,
                        // Binary only if there is an operand to its left.
                        _ => match matches!(st.prev, Prev::Word | Prev::Close) {
                            true => Prev::Op,
                            false => Prev::Prefix,
                        },
                    }
                };
                st.run_role = role;
                st.prev = role;
                st.prev_joint = p.spacing() == Spacing::Joint;
            }
        }
    }
}

/// The inner `T` of the `Account<'info, T>` at account index `i`.
///
/// Only a typed account can carry a data-field operand (`expr::operand` builds `Operand::Field`
/// for nothing else), so the lookup always resolves.
fn inner_ty_at(i: usize, ctx: &crate::expr::ExprCtx) -> syn::Type {
    ctx.inner_ty
        .iter()
        .find(|(nm, _)| ctx.index_of.get(*nm) == Some(&i))
        .map(|(_, t)| t.clone())
        .expect("data-field operand on a field with no typed layout")
}

/// The const-bool guard "every data field this expression reads is present in the USER's Borsh
/// descriptor AND of a type `read_val` can decode", or `None` when the expression reads no data
/// field at all (then it is proven unconditionally).
///
/// WHY THIS IS A CONST AND NOT A BUILD ERROR. `#[derive(AccountData)]` truncates the layout at
/// the first field whose type `map_ty` cannot map, so `constraint = vault.amount >= 1000` over
/// `struct NameVault { name: [u8; N], amount: u64 }` (`N` a named const — M10 Task 15b maps a
/// LITERAL array length, but a const-generic or named-const length is still unevaluable at
/// macro time) reads a field the descriptor does not record. Before M10 Task 13 that was a
/// `const assert!` — a HARD BUILD ERROR on a program real
/// Anchor compiles and enforces, which the prime directive forbids. It cannot be a silent
/// runtime `None` either: `locate` failing rejects EVERY account, bricking the instruction.
///
/// So the decision is deferred to the user's crate, where the descriptor is finally known, and
/// taken by a CONST so both arms are compiled but only one survives: readable => the proven
/// byte-level check in `validate`; not readable => a fallback in `try_accounts`, listed in
/// `UNPROVEN_CHECKS` and absent from `lean_spec`. Enforcement never stops; only the proof does.
///
/// THE SECOND WAY THIS GATE CAN BE WRONG, and the one that made it a CRITICAL finding. Until
/// M10 Task 15b an unmappable field TRUNCATED the descriptor, so "the descriptor names this
/// field" implied "the field is scalar" and a presence test was a sound readability test. Task
/// 15b taught `map_ty` to map `[T; N]`, and an array field became PRESENT while `read_val` kept
/// refusing it. A presence gate then reported `constraint = vault.root == root` as PROVEN, wrote
/// it into the Lean spec, discharged the obligation honestly (Lean's `readVal .array` is `none`
/// as well, so the contract faithfully says "reject everything") — and rejected every account,
/// matching root included. The gate is therefore `has_top_level_scalar_field`, and the same
/// change applies to `String`/`Vec<T>`/`Option<T>`/`Struct` comparisons, all of which real
/// Anchor compiles and enforces.
fn locatability_cond(
    v: &crate::expr::VExpr,
    ctx: &crate::expr::ExprCtx,
    instr_args: &[InstrArg],
) -> Option<TokenStream2> {
    let mut terms: Vec<TokenStream2> = v
        .field_operands()
        .iter()
        .map(|(i, seg)| {
            let inner = inner_ty_at(*i, ctx);
            // SCALAR-READABLE, not merely PRESENT. See `layout::has_top_level_scalar_field`: a
            // present-but-aggregate field (`[u8; 32]`, `String`, `Vec<T>`, `Option<T>`) is one
            // `read_val` refuses, so proving a check over it proves "reject everything".
            quote! {
                ::verified_anchor::layout::has_top_level_scalar_field(
                    <#inner as ::verified_anchor::AccountData>::LAYOUT, #seg)
            }
        })
        .collect();

    // ORDERABILITY — C1's twin. Readability is necessary but NOT sufficient for `<`/`<=`/`>`/
    // `>=`: `evalCmp` orders only what `Value.toInt?` accepts, so a `Pubkey` or `bool` operand
    // makes the ordering `none` — provable, and provably always-rejecting. Equality asks
    // nothing here; `VExpr::ordering_operands` only reports the four orderings.
    for o in v.ordering_operands() {
        match o {
            crate::expr::Orderable::Always => {}
            // Statically non-numeric (`a.key() < b.key()`, `a.is_signer < true`): no descriptor
            // can rescue it, so the whole expression is unprovable in every build.
            crate::expr::Orderable::Never => return Some(quote! { false }),
            // Known at macro time from the declared `#[instruction(..)]` type. `operand()` only
            // builds `InstrArg` for a name in `ctx.instr_args`, which is built from this same
            // list, so a miss here means "declared but not numeric" and is treated as such.
            crate::expr::Orderable::InstrArg(n) => {
                let numeric = instr_args
                    .iter()
                    .any(|a| a.name == n && a.kind == ArgTyKind::Numeric);
                if !numeric {
                    return Some(quote! { false });
                }
            }
            crate::expr::Orderable::Field(i, seg) => {
                let inner = inner_ty_at(i, ctx);
                terms.push(quote! {
                    ::verified_anchor::layout::has_top_level_orderable_field(
                        <#inner as ::verified_anchor::AccountData>::LAYOUT, #seg)
                });
            }
        }
    }

    if terms.is_empty() {
        return None;
    }
    Some(quote! { #(#terms)&&* })
}

/// One `constraint = <expr>` the proof does not (or may not) cover.
struct UnprovenCheck<'a> {
    /// The field the attribute was written on — the `field` of the `ConstraintViolated` error.
    field: String,
    /// The developer's source text, for the error and for `UNPROVEN_CHECKS`.
    src: String,
    /// The expression, run VERBATIM as Rust in `try_accounts`.
    expr: &'a Expr,
    /// `None` — outside the sublanguage, so unproven in every build; the hatch runs the
    /// developer's Rust verbatim. `Some((cond, form))` — INSIDE the sublanguage, but only
    /// provable when `cond` (a const bool evaluated in the user's crate) holds; `form` says
    /// which fallback the hatch runs when it does not.
    ///
    /// `form = None` runs the developer's Rust VERBATIM, exactly like the statically unproven
    /// case. `form = Some(v)` re-evaluates the COMPILED expression `v` against the deserialised
    /// struct through `layout::FieldValue`.
    ///
    /// Neither form is universally usable, which is why the choice is made per expression by
    /// `VExpr::fallback_needs_value_form` (see its doc comment for the rule and its residue):
    ///
    ///   * The compiled form goes through `layout::FieldValue`, which returns `None` for
    ///     `Option<T>`, `Vec<T>`, `String` and `[T; N]` — the very set `read_val` refuses. So it
    ///     BRICKS on exactly the expressions the readability gate now diverts to the hatch, and
    ///     cannot be the fallback for them.
    ///   * The verbatim form is plain Rust, so it cannot express the comparisons where the
    ///     sublanguage is deliberately more permissive than Rust's type checker: `nat` against
    ///     `int` numerically (`vault.delta < vault.amount`, `vault.big > -1`).
    ///
    /// Both arms of the const selection are type-checked regardless of which the const picks,
    /// so this cannot be deferred to the user's crate along with `cond`.
    proven_if: Option<(TokenStream2, Option<crate::expr::VExpr>)>,
}

/// Every constraint expression that must run through the escape hatch, in field order.
///
/// This is the FULL complement of `validate_body`'s expression arm: an expression is either
/// compiled into the proven core there, or it lands here. Nothing may fall between the two —
/// that gap is precisely the silent no-op M10 Task 13 exists to close.
fn unproven_checks<'a>(
    specs: &'a [FieldSpec],
    ctx: &crate::expr::ExprCtx,
    instr_args: &[InstrArg],
) -> Vec<UnprovenCheck<'a>> {
    let mut out = Vec::new();
    for spec in specs {
        for c in &spec.constraints {
            let Constraint::Expr(e) = c else { continue };
            let proven_if = match crate::expr::compile_expr(e, ctx) {
                // Outside the sublanguage (a call, a macro, a module-qualified path, …).
                None => None,
                // Inside it, but its provability depends on the user's descriptor.
                Some(v) => match locatability_cond(&v, ctx, instr_args) {
                    Some(cond) => {
                        let form = match v.fallback_needs_value_form() {
                            true => Some(v),
                            false => None,
                        };
                        Some((cond, form))
                    }
                    // Proven unconditionally — not an escape-hatch check at all.
                    None => continue,
                },
            };
            out.push(UnprovenCheck {
                field: spec.name.clone(),
                src: expr_source(e),
                expr: e,
                proven_if,
            });
        }
    }
    out
}

/// Every identifier appearing in `e` in VALUE POSITION, used to decide which account fields and
/// which `#[instruction(...)]` arguments an escape-hatch expression needs in scope.
///
/// Still deliberately over-approximate — it is a token walk, not name resolution — because the
/// two error directions are not symmetric: binding a name the expression does not use is a
/// warning, while failing to bind one it does use is a hard build failure on valid Anchor.
///
/// The one refinement it does make is dropping identifiers that directly follow an ISOLATED
/// `.`, i.e. field and method names. That is what stops `constraint = at_least(vault.amount)`
/// from claiming to use an `#[instruction(amount: u64)]` argument it never mentions; see
/// `instr_arg_binds`. It cannot introduce a miss, because nothing after a field-separating `.`
/// is ever a name this function's callers would bind: a field name resolves against the
/// receiver, a method name against its impl, and a tuple index is not an `Ident` at all.
///
/// "ISOLATED" IS LOAD-BEARING, and the first cut of this filter got it wrong. A `.` whose
/// PREDECESSOR is also a `.` is the second half of a RANGE (`lo..hi`, `lo..=hi`) or of
/// STRUCT-UPDATE syntax (`Foo { ..base }`), and what follows it is a value-position identifier
/// like any other. Setting the flag on both dots dropped `hi`/`base` from the used set, so the
/// verbatim hatch never bound it and a program REAL ANCHOR COMPILES failed with
/// `error[E0425]: cannot find value `hi` in this scope` — the exact class of build failure this
/// function exists to prevent. (`..=` was already safe by accident: the `=` clears the flag.)
/// `tests/ui/pass/constraint_hatch_range_and_struct_update.rs` is the tripwire.
fn idents_in(e: &Expr) -> std::collections::HashSet<String> {
    fn walk(ts: TokenStream2, out: &mut std::collections::HashSet<String>) {
        let mut after_dot = false;
        // Was the IMMEDIATELY preceding token a `.`? Distinguishes the field-separating `.`
        // from the second dot of `..`/`..=`.
        let mut prev_dot = false;
        for tt in ts {
            match tt {
                proc_macro2::TokenTree::Ident(i) => {
                    if !after_dot {
                        out.insert(i.to_string());
                    }
                    after_dot = false;
                    prev_dot = false;
                }
                proc_macro2::TokenTree::Group(g) => {
                    // A delimited group is never the field/method name of a preceding `.`
                    // (`a.(b)` is not an expression), so the flag does not carry into it.
                    walk(g.stream(), out);
                    after_dot = false;
                    prev_dot = false;
                }
                proc_macro2::TokenTree::Punct(p) => {
                    let dot = p.as_char() == '.';
                    after_dot = dot && !prev_dot;
                    prev_dot = dot;
                }
                proc_macro2::TokenTree::Literal(_) => {
                    after_dot = false;
                    prev_dot = false;
                }
            }
        }
    }
    let mut out = std::collections::HashSet::new();
    walk(quote!(#e), &mut out);
    out
}

/// `lean_constraint`, plus the `constraint = <expr>` arm that needs name resolution.
///
/// An expression OUTSIDE the sublanguage emits NOTHING — not a `compile_error!`. That is the
/// deliberate contract with M10 Task 13: `compile_expr` returning `None` means "the escape hatch
/// owns this one", and until Task 13 lands such an expression is simply absent from both the
/// spec and the runtime checks.
fn lean_constraint_with(c: &Constraint, ctx: &crate::expr::ExprCtx) -> String {
    if let Constraint::Expr(e) = c {
        return match crate::expr::compile_expr(e, ctx) {
            Some(v) => format!("Constraint.expr ({})", v.to_lean()),
            None => String::new(),
        };
    }
    lean_constraint(c)
}

fn lean_constraint(c: &Constraint) -> String {
    match c {
        Constraint::Signer => "Constraint.signer".to_string(),
        Constraint::Mut => "Constraint.mut".to_string(),
        Constraint::Owner(_) => "Constraint.owner ownerPlaceholder".to_string(),
        Constraint::HasOne(t) => format!("Constraint.hasOne \"{}\"", t),
        // Lifecycle markers: not validation constraints; skip in lean_spec output.
        Constraint::InitMarker
        | Constraint::Payer(_)
        | Constraint::Space(_)
        | Constraint::Close(_) => String::new(),
        Constraint::Seeds(elems) => {
            let seeds: Vec<String> = elems
                .iter()
                .map(|se| match se {
                    SeedElem::Literal(b) => {
                        let bytes: Vec<String> = b.value().iter().map(|x| x.to_string()).collect();
                        format!("SeedSpec.literal (ByteArray.mk #[{}])", bytes.join(", "))
                    }
                    SeedElem::FieldKey(id) => format!("SeedSpec.fieldKey \"{}\"", id),
                    SeedElem::InstrArg(off, len) => format!("SeedSpec.instrArg {} {}", off, len),
                    SeedElem::ArgField(id, _) => format!("SeedSpec.argField \"{}\"", id),
                    SeedElem::Unresolved(id) => {
                        unreachable!("unresolved seed `{id}` reached codegen")
                    }
                })
                .collect();
            format!("Constraint.seeds [{}] @@BUMP@@ @@PROG@@", seeds.join(", "))
        }
        // The program override is assembled into the seeds spec's third field below; emit nothing
        // standalone (same pattern as bumps).
        Constraint::SeedsProgram(_) => String::new(),
        Constraint::BumpCanonical | Constraint::BumpDeclared(_) | Constraint::BumpStored(_) => {
            String::new()
        }
        Constraint::Discriminator(d) => {
            let bytes: Vec<String> = d.iter().map(|x| x.to_string()).collect();
            format!(
                "Constraint.discriminator (ByteArray.mk #[{}])",
                bytes.join(", ")
            )
        }
        // Schematic placeholder: the theorem is ∀ over the pubkey (same trick as `owner`).
        Constraint::Address(_) => "Constraint.address Pubkey.zero".to_string(),
        Constraint::Executable => "Constraint.executable".to_string(),
        Constraint::ProgramMarker(_) => String::new(),
        // Not a per-field validation constraint: assembled into the field's `allowDuplicate`
        // list (struct field) in `lean_spec_string`, emitted nothing standalone here.
        Constraint::AllowDuplicate(_) => String::new(),
        // `rent_exempt = enforce` emits the Lean constraint; `skip` emits nothing.
        Constraint::RentExemptEnforce => "Constraint.rentExempt".to_string(),
        Constraint::RentExemptSkip => String::new(),
        // `zero` is a validation constraint (reinit guard); emitted directly in the spec.
        Constraint::Zero => "Constraint.zero".to_string(),
        // Lifecycle markers assembled in lean_spec_string; emit nothing standalone.
        Constraint::Realloc(_)
        | Constraint::ReallocPayer(_)
        | Constraint::ReallocZero(_)
        | Constraint::InitIfNeeded => String::new(),
        // Needs the `ExprCtx` to resolve names; reached only through `lean_constraint_with`.
        Constraint::Expr(_) => String::new(),
    }
}

/// Build the `AccountsStruct` Lean literal for `specs`.
///
/// Returns a `format!` TEMPLATE plus one argument expression per hole, rather than a finished
/// string: an `Account<'info, T>` field's Borsh layout is only knowable in the USER's crate
/// (via `<T as AccountData>::LAYOUT_LEAN`), not here at macro-expansion time. Holes are marked
/// with an INDEXED `@@ARGn@@` sentinel while the literal is assembled and only turned into the
/// POSITIONAL hole `{n}` at the very end, AFTER every literal brace has been escaped —
/// `AccountsStruct` literals are brace-heavy, so escaping first and substituting second is what
/// keeps `format!` from mis-reading `{ name := ... }` as a hole.
///
/// The index is load-bearing, not decoration: with bare `{}` holes each sentinel bound to a
/// pushed argument purely by WALK ORDER, so any future edit that reordered field emission would
/// silently splice one type's layout under another type's name — a wrong Lean spec that still
/// compiles. `@@ARGn@@` ties each hole to the argument it was created with, so the binding
/// survives reordering. `tests/lean_spec.rs::lean_spec_splices_the_real_layout` is the tripwire.
fn lean_spec_string(specs: &[FieldSpec], instr_args: &[InstrArg]) -> (String, Vec<TokenStream2>) {
    let mut fields = Vec::new();
    let mut args: Vec<TokenStream2> = Vec::new();
    // Same resolution the runtime checks use — see `expr_maps`.
    let (expr_index_of, expr_inner_ty) = expr_maps(specs);
    let expr_arg_names: Vec<String> = instr_args.iter().map(|a| a.name.clone()).collect();
    let expr_ctx = crate::expr::ExprCtx {
        index_of: &expr_index_of,
        inner_ty: &expr_inner_ty,
        instr_args: &expr_arg_names,
        deser: false,
    };
    for spec in specs {
        // An entry is either a fixed string, or — for a `constraint = <expr>` whose data-field
        // reads may not be locatable in the user's descriptor — a string chosen by a CONST in
        // the user's crate: the Lean literal when the expression is provable there, `""` when
        // it is not and the escape hatch owns it instead. See `locatability_cond`.
        let mut cs_cond: Vec<TokenStream2> = Vec::new();
        let cs: Vec<String> = spec
            .constraints
            .iter()
            .map(|c| {
                let s = lean_constraint_with(c, &expr_ctx);
                if let Constraint::Expr(e) = c {
                    if !s.is_empty() {
                        if let Some(v) = crate::expr::compile_expr(e, &expr_ctx) {
                            if let Some(cond) = locatability_cond(&v, &expr_ctx, instr_args) {
                                cs_cond.push(quote! { if #cond { #s } else { "" } });
                                // Placeholder: spliced back in below as a runtime hole.
                                return "@@COND@@".to_string();
                            }
                        }
                    }
                }
                s
            })
            .filter(|s| !s.is_empty())
            .collect();
        let mut cs = cs; // make mutable
                         // init: assemble InitMarker + Payer + Space -> Constraint.init "<payer>" <space> Pubkey.zero
        if spec
            .constraints
            .iter()
            .any(|c| matches!(c, Constraint::InitMarker))
        {
            let payer = spec.constraints.iter().find_map(|c| {
                if let Constraint::Payer(p) = c {
                    Some(p.to_string())
                } else {
                    None
                }
            });
            let space = spec.constraints.iter().find_map(|c| {
                if let Constraint::Space(n) = c {
                    Some(*n)
                } else {
                    None
                }
            });
            if let (Some(payer), Some(space)) = (payer, space) {
                cs.push(format!(
                    "Constraint.init \"{}\" {} Pubkey.zero",
                    payer, space
                ));
            }
        }
        // close: Close(dest) -> Constraint.close "<dest>"
        if let Some(dest) = spec.constraints.iter().find_map(|c| {
            if let Constraint::Close(d) = c {
                Some(d.to_string())
            } else {
                None
            }
        }) {
            cs.push(format!("Constraint.close \"{}\"", dest));
        }
        // realloc: Realloc(newLen) + ReallocPayer(p) [+ ReallocZero(z)] -> Constraint.realloc "<p>" <newLen> <z>
        if let Some(newlen) = spec.constraints.iter().find_map(|c| {
            if let Constraint::Realloc(n) = c {
                Some(*n)
            } else {
                None
            }
        }) {
            let payer = spec.constraints.iter().find_map(|c| {
                if let Constraint::ReallocPayer(p) = c {
                    Some(p.to_string())
                } else {
                    None
                }
            });
            let zero = spec
                .constraints
                .iter()
                .any(|c| matches!(c, Constraint::ReallocZero(true)));
            if let Some(payer) = payer {
                cs.push(format!(
                    "Constraint.realloc \"{}\" {} {}",
                    payer, newlen, zero
                ));
            }
        }
        // init_if_needed: InitIfNeeded + Payer + Space -> Constraint.initIfNeeded "<payer>" <space> Pubkey.zero
        if spec
            .constraints
            .iter()
            .any(|c| matches!(c, Constraint::InitIfNeeded))
        {
            let payer = spec.constraints.iter().find_map(|c| {
                if let Constraint::Payer(p) = c {
                    Some(p.to_string())
                } else {
                    None
                }
            });
            let space = spec.constraints.iter().find_map(|c| {
                if let Constraint::Space(n) = c {
                    Some(*n)
                } else {
                    None
                }
            });
            if let (Some(payer), Some(space)) = (payer, space) {
                cs.push(format!(
                    "Constraint.initIfNeeded \"{}\" {} Pubkey.zero",
                    payer, space
                ));
            }
        }
        let ty = match &spec.kind {
            WrapperKind::Account(t) => {
                // Both holes are filled at RUNTIME from the inner type: the type name (so the
                // Lean discriminator matches) and its `LAYOUT_LEAN`, so the emitted literal
                // carries the struct's real field offsets rather than the pre-M10
                // `[("<has_one target>", 8)]`, which pinned every target to the first field.
                let tstr = t.to_string();
                let name_hole = args.len();
                args.push(quote! { #tstr });
                let layout_hole = args.len();
                args.push(quote! { <#t as ::verified_anchor::AccountData>::LAYOUT_LEAN });
                // LAYOUT_LEAN is already parenthesised, so it drops straight into the
                // `layout : Ty` position without adding parens here.
                format!(
                    "AccountType.account \"@@ARG{name_hole}@@\" @@ARG{layout_hole}@@ Pubkey.zero"
                )
            }
            WrapperKind::Signer => "AccountType.signer".to_string(),
            WrapperKind::SystemAccount => "AccountType.systemAccount".to_string(),
            // `Program<P>` implies `executable` + `address = P::ID` in Lean; the concrete id
            // is unknown at macro time, so emit the schematic placeholder `Pubkey.zero`.
            WrapperKind::Program(_) => "AccountType.program Pubkey.zero".to_string(),
            WrapperKind::Unchecked => "AccountType.uncheckedAccount".to_string(),
        };
        // Parenthesise bumps that carry an argument so the emitted spec parses as a single
        // `BumpSpec` argument of `Constraint.seeds` (canonical takes no arg, so no parens).
        let bump_str = spec
            .constraints
            .iter()
            .find_map(|c| match c {
                Constraint::BumpCanonical => Some("BumpSpec.canonical".to_string()),
                Constraint::BumpDeclared(d) => Some(format!("(BumpSpec.declared {})", d)),
                Constraint::BumpStored(off) => Some(format!("(BumpSpec.stored {})", off)),
                _ => None,
            })
            .unwrap_or_else(|| "BumpSpec.canonical".to_string());
        // `seeds::program` override → the third `Constraint.seeds` field. Present ⇒ the schematic
        // placeholder `(some Pubkey.zero)` (the soundness theorem is ∀ over the pubkey, exactly
        // like `owner`/`address`); absent ⇒ `none` (derive against this program's id).
        let prog_str = if spec
            .constraints
            .iter()
            .any(|c| matches!(c, Constraint::SeedsProgram(_)))
        {
            "(some Pubkey.zero)"
        } else {
            "none"
        };
        let cs_joined = if cs_cond.is_empty() {
            cs.join(", ")
                .replace("@@BUMP@@", &bump_str)
                .replace("@@PROG@@", prog_str)
        } else {
            // At least one entry is decided in the user's crate, so the whole constraint list
            // is joined at RUNTIME: an entry that resolves to `""` must vanish along with its
            // separator, or the literal would come out as `[Constraint.mut, ]` and not parse.
            let mut conds = cs_cond.into_iter();
            let pieces: Vec<TokenStream2> = cs
                .iter()
                .map(|e| match e.as_str() {
                    "@@COND@@" => conds.next().expect("one hole per conditional entry"),
                    lit => {
                        let lit = lit
                            .replace("@@BUMP@@", &bump_str)
                            .replace("@@PROG@@", prog_str);
                        quote! { #lit }
                    }
                })
                .collect();
            let hole = args.len();
            args.push(quote! {
                {
                    let __cs: &[&str] = &[#(#pieces),*];
                    __cs.iter()
                        .filter(|s| !s.is_empty())
                        .cloned()
                        .collect::<::std::vec::Vec<&str>>()
                        .join(", ")
                }
            });
            format!("@@ARG{hole}@@")
        };
        // `allow_duplicate = <field>` opt-outs → the field's `allowDuplicate` list. Emitted
        // ONLY when non-empty so existing literals keep relying on the Lean field default `[]`.
        let allows: Vec<String> = spec
            .constraints
            .iter()
            .filter_map(|c| match c {
                Constraint::AllowDuplicate(t) => Some(format!("\"{}\"", t)),
                _ => None,
            })
            .collect();
        let allow_str = if allows.is_empty() {
            String::new()
        } else {
            format!(", allowDuplicate := [{}]", allows.join(", "))
        };
        fields.push(format!(
            "{{ name := \"{}\", ty := {}, constraints := [{}]{} }}",
            spec.name, ty, cs_joined, allow_str
        ));
    }
    let body = if fields.is_empty() {
        "[]".to_string()
    } else {
        let mut lines = String::from("\n  [ ");
        lines.push_str(&fields[0]);
        for f in &fields[1..] {
            lines.push_str("\n  , ");
            lines.push_str(f);
        }
        lines.push_str(" ]");
        lines
    };
    // `instrArgs` is emitted ONLY when non-empty, so every pre-M10 spec is byte-identical to
    // before and keeps relying on the Lean field default `[]` (same rule as `allowDuplicate`).
    let instr_args_str = if instr_args.is_empty() {
        String::new()
    } else {
        format!(
            ", instrArgs := [{}]",
            instr_args
                .iter()
                .map(|a| format!("(\"{}\", {})", a.name, a.lean))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let raw = format!(
        "{{ programId := Pubkey.zero{}, fields :={} }}",
        instr_args_str, body
    );
    // Escape LITERAL braces first (the record syntax `{ name := .. }` is not a format hole),
    // then turn the sentinels into holes. Doing it in this order is load-bearing: escaping
    // after substitution would double up the `{}` we just introduced. The `@@` delimiters make
    // the per-index replacements unambiguous (`@@ARG1@@` cannot match inside `@@ARG11@@`).
    let mut tpl = raw.replace('{', "{{").replace('}', "}}");
    for i in 0..args.len() {
        tpl = tpl.replace(&format!("@@ARG{i}@@"), &format!("{{{i}}}"));
    }
    (tpl, args)
}

fn validate_body(specs: &[FieldSpec], instr_args: &[InstrArg]) -> TokenStream2 {
    let n = specs.len();
    // name→index (has_one / seed targets) and name→inner type (`constraint = <expr>` data
    // fields), built together so the runtime checks and the Lean spec resolve names identically.
    let (index_of, inner_ty) = expr_maps(specs);
    let expr_arg_names: Vec<String> = instr_args.iter().map(|a| a.name.clone()).collect();
    let expr_ctx = crate::expr::ExprCtx {
        index_of: &index_of,
        inner_ty: &inner_ty,
        instr_args: &expr_arg_names,
        deser: false,
    };
    // Carried forward from M10 Task 9: `INSTR_ARGS` used to be emitted only when a SEED
    // referenced an instruction argument. `Operand::InstrArg`'s codegen references the same
    // const, so the gate is widened here to "a seed needs it OR a compiled expression reads an
    // argument". Emitting it unconditionally is not an option — an unused local `const` raises
    // `dead_code` in every user crate that has neither.
    let mut needs_instr_args = uses_arg_field(specs);
    let mut checks = Vec::new();
    for (i, spec) in specs.iter().enumerate() {
        let name = &spec.name;
        let implied = wrapper_implied(&spec.kind);
        // init_if_needed drop-in: a FRESH (system-owned, zero-disc) account cannot pass the
        // wrapper-implied Owner(crate::ID)/Discriminator checks, which would block the very
        // init path init_if_needed exists for. So for an iin field we filter OUT the
        // wrapper-IMPLIED Owner/Discriminator here; the reinit guard (owner + size on an
        // already-initialized account) is re-established in execute_lifecycle's iin ELSE
        // branch, matching the proven Lean `applyInitIfNeeded`. Explicit seeds/address/etc.
        // constraints are KEPT — they identify the account and hold on fresh AND existing.
        let has_iin = spec
            .constraints
            .iter()
            .any(|c| matches!(c, Constraint::InitIfNeeded));
        let effective: Vec<Constraint> = implied
            .into_iter()
            .filter(|c| {
                !(has_iin && matches!(c, Constraint::Owner(_) | Constraint::Discriminator(_)))
            })
            .chain(spec.constraints.iter().cloned())
            .collect();
        for c in &effective {
            let check = match c {
                Constraint::Signer => quote! {
                    if !accounts[#i].is_signer {
                        return Err(::verified_anchor::VAError::MissingSigner { field: #name });
                    }
                },
                Constraint::Mut => quote! {
                    if !accounts[#i].is_writable {
                        return Err(::verified_anchor::VAError::NotWritable { field: #name });
                    }
                },
                Constraint::Owner(expr) => quote! {
                    if accounts[#i].owner != &(#expr) {
                        return Err(::verified_anchor::VAError::WrongOwner { field: #name });
                    }
                },
                Constraint::HasOne(target) => {
                    let tname = target.to_string();
                    let tidx = *index_of.get(&tname).unwrap_or_else(|| {
                        panic!("has_one target `{tname}` is not a field of this struct")
                    });
                    let fname = name;
                    // Only `Account<'info, T>` carries a layout; anything else has no modelled
                    // fields and must fail closed rather than silently reading offset 8 (the
                    // pre-M10 behaviour, which compared the struct's FIRST field whatever field
                    // the developer named). `has_one` on an untyped wrapper is not valid Anchor
                    // either, and Lean's `AccountType.locateField` returns `none` for non-account
                    // wrappers, so the model cannot express it.
                    let inner = match &spec.kind {
                        WrapperKind::Account(t) => t.clone(),
                        // Unreachable: `derive_verified_accounts` rejects this up front. Kept
                        // so the arm fails closed if that guard is ever relaxed.
                        _ => return syn::Error::new(
                            target.span(),
                            "verified-anchor: `has_one` requires a typed `Account<'info, T>` field so the \
                             target's Borsh offset is known",
                        ).to_compile_error(),
                    };
                    quote! {
                        {
                            // BUILD-TIME guard. `locate` needs the target to be present in the
                            // descriptor; when it is not, the runtime check below rejects EVERY
                            // account, including legitimate ones, and does so silently. That can
                            // happen to correct-looking Anchor code: `#[derive(AccountData)]`
                            // truncates the layout at the first field whose type `map_ty` cannot
                            // map (a non-literal-length array, a nested struct, an enum, ...),
                            // because every offset behind such a field is unknowable. Deciding
                            // this from the descriptor alone turns a bricked instruction into a
                            // build error.
                            const _: () = ::core::assert!(
                                ::verified_anchor::layout::has_top_level_field(
                                    <#inner as ::verified_anchor::AccountData>::LAYOUT, #tname),
                                ::core::concat!(
                                    "verified-anchor: `has_one = ", #tname,
                                    "` cannot be located in the Borsh layout of `", ::core::stringify!(#inner),
                                    "`. Either `", #tname, "` is not a field of `", ::core::stringify!(#inner),
                                    "`, or an EARLIER field of `", ::core::stringify!(#inner),
                                    "` has a type verified-anchor cannot map yet (an array with a non-literal \
                                     length, a nested struct, an enum, ...): the layout is truncated at the \
                                     first such field, because \
                                     every offset behind it is unknowable. Move the target ahead of that field, \
                                     or give that field a mappable type."),
                            );
                            // Same failure profile, different cause: a non-`Pubkey` target makes
                            // `read_val` yield a non-`Key` value, so the check below would also
                            // reject unconditionally. `has_one` on a non-key field is not valid
                            // stock Anchor either. Short-circuited on the previous assertion's
                            // condition so an ABSENT target reports only the (more informative)
                            // truncation diagnostic, not both.
                            const _: () = ::core::assert!(
                                !::verified_anchor::layout::has_top_level_field(
                                    <#inner as ::verified_anchor::AccountData>::LAYOUT, #tname)
                                || ::verified_anchor::layout::has_top_level_pubkey_field(
                                    <#inner as ::verified_anchor::AccountData>::LAYOUT, #tname),
                                ::core::concat!(
                                    "verified-anchor: `has_one = ", #tname, "` requires `",
                                    ::core::stringify!(#inner), "::", #tname,
                                    "` to be a `Pubkey`; `has_one` compares it against an account key."),
                            );
                            let data = accounts[#i].try_borrow_data()
                                .map_err(|_| ::verified_anchor::VAError::WrongHasOne { field: #fname, target: #tname })?;
                            let ty = <#inner as ::verified_anchor::AccountData>::LAYOUT;
                            // Walk the real Borsh layout from offset 8 (past the discriminator).
                            let found = ::verified_anchor::layout::locate(&ty, &[#tname], &data, 8)
                                .and_then(|(off, fty)| ::verified_anchor::layout::read_val(&fty, &data, off));
                            match found {
                                Some(::verified_anchor::layout::Value::Key(k)) if k == *accounts[#tidx].key => {}
                                _ => return Err(::verified_anchor::VAError::WrongHasOne { field: #fname, target: #tname }),
                            }
                        }
                    }
                }
                Constraint::Discriminator(disc) => {
                    let fname = name;
                    let bs: Vec<u8> = disc.to_vec();
                    quote! {
                        {
                            let data = accounts[#i].try_borrow_data()
                                .map_err(|_| ::verified_anchor::VAError::WrongDiscriminator { field: #fname })?;
                            const __DISC: [u8; 8] = [#(#bs),*];
                            if data.len() < 8 || data[0..8] != __DISC {
                                return Err(::verified_anchor::VAError::WrongDiscriminator { field: #fname });
                            }
                        }
                    }
                }
                Constraint::Address(expr) => quote! {
                    if accounts[#i].key != &(#expr) {
                        return Err(::verified_anchor::VAError::WrongAddress { field: #name });
                    }
                },
                Constraint::Executable => quote! {
                    if !accounts[#i].executable {
                        return Err(::verified_anchor::VAError::NotExecutable { field: #name });
                    }
                },
                Constraint::ProgramMarker(p) => {
                    let fname = name;
                    let pid_ty = p;
                    quote! {
                        if !accounts[#i].executable {
                            return Err(::verified_anchor::VAError::WrongOwner { field: #fname });
                        }
                        if accounts[#i].key != &<#pid_ty as ::verified_anchor::ProgramId>::ID {
                            return Err(::verified_anchor::VAError::WrongOwner { field: #fname });
                        }
                    }
                }
                Constraint::RentExemptEnforce => {
                    let fname = name;
                    quote! {
                        {
                            use ::verified_anchor::solana_program::sysvar::Sysvar as _;
                            let __rent = ::verified_anchor::solana_program::rent::Rent::get()
                                .map_err(|_| ::verified_anchor::VAError::NotRentExempt { field: #fname })?;
                            if !__rent.is_exempt(accounts[#i].lamports(), accounts[#i].data_len()) {
                                return Err(::verified_anchor::VAError::NotRentExempt { field: #fname });
                            }
                        }
                    }
                }
                // Lifecycle markers are handled in execute_lifecycle, not validate.
                Constraint::InitMarker
                | Constraint::Payer(_)
                | Constraint::Space(_)
                | Constraint::Close(_) => {
                    continue;
                }
                // Seeds/bump/seeds::program are handled in the per-field PDA block below.
                Constraint::Seeds(_)
                | Constraint::SeedsProgram(_)
                | Constraint::BumpCanonical
                | Constraint::BumpDeclared(_)
                | Constraint::BumpStored(_) => {
                    continue;
                }
                // The opt-out tunes the struct-level pairwise check below, not a per-field check.
                Constraint::AllowDuplicate(_) => {
                    continue;
                }
                // skip emits nothing — documented SAFE-BY-DEFAULT opt-out.
                Constraint::RentExemptSkip => {
                    continue;
                }
                // Lifecycle markers (handled in execute_lifecycle, not validate).
                Constraint::Realloc(_)
                | Constraint::ReallocPayer(_)
                | Constraint::ReallocZero(_)
                | Constraint::InitIfNeeded => {
                    continue;
                }
                // `constraint = <expr>`: compile into the proven sublanguage, or hand it to the
                // escape hatch. `compile_expr` returning `None` is NOT an error — real Anchor
                // accepts arbitrary expressions, so a `compile_error!` here would break the
                // drop-in property. Such an expression is emitted by `derive_verified_accounts`
                // into `try_accounts` instead (M10 Task 13), where the deserialised bindings it
                // needs exist; `validate` is the byte-level path and cannot run it. `continue`
                // here therefore drops NOTHING — see `unproven_checks`.
                Constraint::Expr(e) => {
                    match crate::expr::compile_expr(e, &expr_ctx) {
                        Some(v) => {
                            if v.uses_instr_arg() {
                                needs_instr_args = true;
                            }
                            let check = v.to_tokens_check(name, &expr_source(e), &expr_ctx);
                            // The const-selected branch: when a data field this expression
                            // reads is missing from the user's descriptor, the proven check
                            // would reject every account, so it is switched OFF here and the
                            // developer's verbatim Rust runs in `try_accounts` instead. See
                            // `locatability_cond`.
                            match locatability_cond(&v, &expr_ctx, instr_args) {
                                Some(cond) => quote! { if #cond { #check } },
                                None => check,
                            }
                        }
                        None => continue,
                    }
                }
                // `zero`: reinit guard — checks the first 8 bytes are all-zero.
                Constraint::Zero => {
                    let fname = name;
                    quote! {
                        {
                            let data = accounts[#i].try_borrow_data()
                                .map_err(|_| ::verified_anchor::VAError::NotZeroed { field: #fname })?;
                            if data.len() < 8 || data[0..8] != [0u8; 8] {
                                return Err(::verified_anchor::VAError::NotZeroed { field: #fname });
                            }
                        }
                    }
                }
            };
            checks.push(check);
        }

        // seeds/bump: emit one PDA check per field that declares `seeds`.
        if let Some(Constraint::Seeds(elems)) = spec
            .constraints
            .iter()
            .find(|c| matches!(c, Constraint::Seeds(_)))
        {
            let fname = name;
            // `seeds::program = <expr>` override: derive against the FOREIGN program id `<expr>`
            // (a `Pubkey` value) instead of this program's `program_id` (already a `&Pubkey`).
            let derive_pid: TokenStream2 = match spec.constraints.iter().find_map(|c| match c {
                Constraint::SeedsProgram(e) => Some(e),
                _ => None,
            }) {
                Some(expr) => quote! { &(#expr) },
                None => quote! { program_id },
            };
            let seed_exprs: Vec<TokenStream2> = elems.iter().map(|se| match se {
                SeedElem::Literal(b) => quote! { &#b[..] },
                SeedElem::FieldKey(id) => {
                    // Guarded with a span in `derive_verified_accounts`, which runs first.
                    let fi = *index_of.get(&id.to_string())
                        .unwrap_or_else(|| unreachable!("seed field `{id}` reached codegen"));
                    quote! { accounts[#fi].key.as_ref() }
                }
                SeedElem::InstrArg(off, len) => {
                    let end = off + len;
                    // Clamp to length so a short `instr_data` cannot panic; this mirrors the
                    // Lean model's `ByteArray.extract off (off+len)` (which clamps both bounds).
                    quote! { &instr_data[(#off).min(instr_data.len())..(#end).min(instr_data.len())] }
                }
                SeedElem::ArgField(id, _) => arg_field_seed_expr(id, fname),
                SeedElem::Unresolved(id) => unreachable!("unresolved seed `{id}` reached codegen"),
            }).collect();
            // Stored (non-canonical) bump opt-in: `bump = arg(off)`. Read the bump byte from
            // instr_data at `off`, derive the PDA with THAT specific bump via
            // create_program_address, compare to the account key. NO canonical requirement.
            let stored_off = spec.constraints.iter().find_map(|c| match c {
                Constraint::BumpStored(off) => Some(*off),
                _ => None,
            });
            if let Some(off) = stored_off {
                checks.push(quote! {
                    {
                        let __seeds: &[&[u8]] = &[ #(#seed_exprs),* ];
                        // None-safe: short instr_data (no byte at `off`) is a clean reject,
                        // mirroring the Lean spec's `instrData.data[off]?` none case.
                        let __stored_bump = match instr_data.get(#off) {
                            ::core::option::Option::Some(b) => *b,
                            ::core::option::Option::None =>
                                return Err(::verified_anchor::VAError::WrongPda { field: #fname }),
                        };
                        // create_program_address fails (Err) for an on-curve candidate; that is
                        // also a clean reject (mirrors the Lean `createProgramAddress = none`).
                        let __pda = match ::verified_anchor::solana_program::pubkey::Pubkey::create_program_address(
                            &[ #(#seed_exprs,)* &[__stored_bump] ], #derive_pid)
                        {
                            ::core::result::Result::Ok(pk) => pk,
                            ::core::result::Result::Err(_) =>
                                return Err(::verified_anchor::VAError::WrongPda { field: #fname }),
                        };
                        let _ = __seeds;
                        if accounts[#i].key != &__pda {
                            return Err(::verified_anchor::VAError::WrongPda { field: #fname });
                        }
                    }
                });
            } else {
                let bump_check = match spec.constraints.iter().find_map(|c| match c {
                    Constraint::BumpCanonical => Some(None),
                    Constraint::BumpDeclared(d) => Some(Some(*d)),
                    _ => None,
                }) {
                    Some(Some(d)) => quote! {
                        if __bump != #d {
                            return Err(::verified_anchor::VAError::WrongBump { field: #fname });
                        }
                    },
                    _ => quote! {},
                };
                checks.push(quote! {
                    {
                        let __seeds: &[&[u8]] = &[ #(#seed_exprs),* ];
                        let (__pda, __bump) = ::verified_anchor::solana_program::pubkey::Pubkey::find_program_address(__seeds, #derive_pid);
                        if accounts[#i].key != &__pda {
                            return Err(::verified_anchor::VAError::WrongPda { field: #fname });
                        }
                        #bump_check
                    }
                });
            }
        }
    }

    // STRUCT-LEVEL distinct-mut-key check (M8.4): every ordered pair of `mut` fields `i < j`
    // that is NOT opted out must resolve to distinct account keys. Mirrors the Lean
    // `distinctMutKeys` (mut = `Constraint::Mut` in implied++explicit; exempt = either field
    // lists the other in `allow_duplicate`).
    let is_mut = |spec: &FieldSpec| -> bool {
        wrapper_implied(&spec.kind)
            .iter()
            .chain(spec.constraints.iter())
            .any(|c| matches!(c, Constraint::Mut))
    };
    let allows = |spec: &FieldSpec, other: &str| -> bool {
        spec.constraints
            .iter()
            .any(|c| matches!(c, Constraint::AllowDuplicate(t) if t == other))
    };
    let mut_indices: Vec<usize> = specs
        .iter()
        .enumerate()
        .filter(|(_, s)| is_mut(s))
        .map(|(i, _)| i)
        .collect();
    for (a, &i) in mut_indices.iter().enumerate() {
        for &j in &mut_indices[a + 1..] {
            let exempt = allows(&specs[i], &specs[j].name) || allows(&specs[j], &specs[i].name);
            if exempt {
                continue;
            }
            let fa = &specs[i].name;
            let fb = &specs[j].name;
            checks.push(quote! {
                if accounts[#i].key == accounts[#j].key {
                    return Err(::verified_anchor::VAError::DuplicateAccount { field_a: #fa, field_b: #fb });
                }
            });
        }
    }

    // Emitted only when a `name.as_bytes()` seed or a compiled `constraint = <expr>` actually
    // needs it: an unused local `const` would raise `dead_code` in every user crate that has
    // neither. See `needs_instr_args` above.
    let instr_args_decl = if needs_instr_args {
        instr_args_const(instr_args)
    } else {
        quote! {}
    };

    quote! {
        fn validate(
            accounts: &[::verified_anchor::solana_program::account_info::AccountInfo],
            instr_data: &[u8],
            program_id: &::verified_anchor::solana_program::pubkey::Pubkey,
        ) -> ::core::result::Result<(), ::verified_anchor::VAError> {
            #instr_args_decl
            let _ = (instr_data, program_id);
            if accounts.len() < #n {
                return Err(::verified_anchor::VAError::NotEnoughAccounts { expected: #n, got: accounts.len() });
            }
            #(#checks)*
            Ok(())
        }
    }
}

fn lifecycle_body(specs: &[FieldSpec]) -> TokenStream2 {
    let n = specs.len();
    // Build name→index map for resolving payer/dest references.
    let index_of: std::collections::HashMap<String, usize> = specs
        .iter()
        .enumerate()
        .map(|(i, s)| (s.name.clone(), i))
        .collect();

    let mut lifecycle_steps: Vec<TokenStream2> = Vec::new();

    for (i, spec) in specs.iter().enumerate() {
        let fname = &spec.name;

        // Detect init: requires InitMarker + Payer + Space all present.
        let has_init = spec
            .constraints
            .iter()
            .any(|c| matches!(c, Constraint::InitMarker));
        if has_init {
            let payer_ident = spec.constraints.iter().find_map(|c| {
                if let Constraint::Payer(p) = c {
                    Some(p.to_string())
                } else {
                    None
                }
            });
            let space_val = spec.constraints.iter().find_map(|c| {
                if let Constraint::Space(n) = c {
                    Some(*n)
                } else {
                    None
                }
            });
            if let (Some(payer_name), Some(n)) = (payer_ident, space_val) {
                let pi = *index_of.get(&payer_name).unwrap_or_else(|| {
                    panic!("init payer `{payer_name}` is not a field of this struct")
                });
                lifecycle_steps.push(quote! {
                    {
                        let space_total: usize = #n + 8;
                        let ix = ::verified_anchor::solana_program::system_instruction::create_account(
                            accounts[#pi].key, accounts[#i].key, rent_lamports, space_total as u64, program_id);
                        ::verified_anchor::solana_program::program::invoke(&ix, accounts)
                            .map_err(|_| ::verified_anchor::VAError::InitFailed { field: #fname })?;
                        let mut d = accounts[#i].try_borrow_mut_data()
                            .map_err(|_| ::verified_anchor::VAError::InitFailed { field: #fname })?;
                        for b in d.iter_mut().take(8) { *b = 0; }
                    }
                });
            }
        }

        // Detect close: requires Close(dest).
        let close_dest = spec.constraints.iter().find_map(|c| {
            if let Constraint::Close(dest) = c {
                Some(dest.to_string())
            } else {
                None
            }
        });
        if let Some(dest_name) = close_dest {
            let di = *index_of.get(&dest_name).unwrap_or_else(|| {
                panic!("close destination `{dest_name}` is not a field of this struct")
            });
            lifecycle_steps.push(quote! {
                {
                    let bal = accounts[#i].lamports();
                    **accounts[#di].try_borrow_mut_lamports()
                        .map_err(|_| ::verified_anchor::VAError::CloseFailed { field: #fname })? += bal;
                    **accounts[#i].try_borrow_mut_lamports()
                        .map_err(|_| ::verified_anchor::VAError::CloseFailed { field: #fname })? = 0;
                    let mut d = accounts[#i].try_borrow_mut_data()
                        .map_err(|_| ::verified_anchor::VAError::CloseFailed { field: #fname })?;
                    for b in d.iter_mut().take(8) { *b = 0xff; }
                }
            });
        }

        // realloc
        let realloc_newlen = spec.constraints.iter().find_map(|c| {
            if let Constraint::Realloc(n) = c {
                Some(*n)
            } else {
                None
            }
        });
        if let Some(newlen) = realloc_newlen {
            let payer_name = spec
                .constraints
                .iter()
                .find_map(|c| {
                    if let Constraint::ReallocPayer(p) = c {
                        Some(p.to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| panic!("realloc on `{}` needs realloc::payer", fname));
            let pi = *index_of.get(&payer_name).unwrap_or_else(|| {
                panic!("realloc::payer `{payer_name}` is not a field of this struct")
            });
            let zero_flag = spec
                .constraints
                .iter()
                .any(|c| matches!(c, Constraint::ReallocZero(true)));
            lifecycle_steps.push(quote! {
                {
                    use ::verified_anchor::solana_program::sysvar::Sysvar as _;
                    let __rent = ::verified_anchor::solana_program::rent::Rent::get()
                        .map_err(|_| ::verified_anchor::VAError::ReallocFailed { field: #fname })?;
                    let __min = __rent.minimum_balance(#newlen);
                    let __cur = accounts[#i].lamports();
                    if __min > __cur {
                        let __delta = __min.saturating_sub(__cur);
                        // top-up: payer -> account (payer is a writable signer; system-owned)
                        let __ix = ::verified_anchor::solana_program::system_instruction::transfer(
                            accounts[#pi].key, accounts[#i].key, __delta);
                        ::verified_anchor::solana_program::program::invoke(&__ix, accounts)
                            .map_err(|_| ::verified_anchor::VAError::ReallocFailed { field: #fname })?;
                    }
                    accounts[#i].realloc(#newlen, #zero_flag)
                        .map_err(|_| ::verified_anchor::VAError::ReallocFailed { field: #fname })?;
                }
            });
        }

        // init_if_needed
        let has_iin = spec
            .constraints
            .iter()
            .any(|c| matches!(c, Constraint::InitIfNeeded));
        if has_iin {
            let payer_name = spec
                .constraints
                .iter()
                .find_map(|c| {
                    if let Constraint::Payer(p) = c {
                        Some(p.to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| panic!("init_if_needed on `{}` needs payer", fname));
            let pi = *index_of
                .get(&payer_name)
                .unwrap_or_else(|| panic!("init_if_needed payer `{payer_name}` is not a field"));
            let n = spec
                .constraints
                .iter()
                .find_map(|c| {
                    if let Constraint::Space(n) = c {
                        Some(*n)
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| panic!("init_if_needed on `{}` needs space", fname));
            // The Task 6 struct-level guard requires an iin field to be a typed
            // `Account<'info, T>`; extract T so we can stamp its real Anchor discriminator
            // (making the fresh account a valid, type-correct, re-detectable account) and
            // reject a wrong-owner/undersized existing account in the ELSE branch.
            let t = match &spec.kind {
                WrapperKind::Account(t) => t.clone(),
                _ => panic!(
                    "init_if_needed on `{}` must be a typed Account (Task 6 guard)",
                    fname
                ),
            };
            // Seeded-PDA iin fields must be created with `invoke_signed` (the PDA is off-curve
            // and cannot sign as a tx signer). Re-derive the canonical bump inside
            // `execute_lifecycle` from the field's seed literals/field-keys and pass the signer
            // seeds. `arg(..)` seeds are NOT supported here — execute_lifecycle has no instr_data
            // — so reject them at compile time with a clear message.
            let seeds_for_signed: Option<Vec<TokenStream2>> = spec
                .constraints
                .iter()
                .find_map(|c| {
                    if let Constraint::Seeds(elems) = c {
                        Some(elems)
                    } else {
                        None
                    }
                })
                .map(|elems| {
                    elems
                        .iter()
                        .map(|se| match se {
                            SeedElem::Literal(b) => quote! { &#b[..] },
                            SeedElem::FieldKey(id) => {
                                // Guarded with a span in `derive_verified_accounts`, which runs first.
                                let fi = *index_of.get(&id.to_string()).unwrap_or_else(|| {
                                    unreachable!("seed field `{id}` reached codegen")
                                });
                                quote! { accounts[#fi].key.as_ref() }
                            }
                            SeedElem::InstrArg(off, len) => unreachable!(
                        "init_if_needed + `arg({off}, {len})` seed on `{fname}` reached codegen"),
                            SeedElem::Unresolved(id) => {
                                unreachable!("unresolved seed `{id}` reached codegen")
                            }
                            // Both instruction-data seed kinds are rejected with a span by the
                            // init_if_needed guard in `derive_verified_accounts`, which runs first.
                            SeedElem::ArgField(id, _) => unreachable!(
                                "init_if_needed + `{id}` arg seed on `{fname}` reached codegen"
                            ),
                        })
                        .collect()
                });
            let init_create = match &seeds_for_signed {
                Some(seed_exprs) => quote! {
                    // Seeded PDA: derive the canonical bump, then invoke_signed so the PDA can
                    // authorize its own creation.
                    let __seeds: &[&[u8]] = &[ #(#seed_exprs),* ];
                    let (__pda, __bump) = ::verified_anchor::solana_program::pubkey::Pubkey::find_program_address(__seeds, program_id);
                    if accounts[#i].key != &__pda {
                        return Err(::verified_anchor::VAError::InitFailed { field: #fname });
                    }
                    let __signer_seeds: &[&[u8]] = &[ #(#seed_exprs,)* &[__bump] ];
                    ::verified_anchor::solana_program::program::invoke_signed(&ix, accounts, &[__signer_seeds])
                        .map_err(|_| ::verified_anchor::VAError::InitFailed { field: #fname })?;
                },
                None => quote! {
                    ::verified_anchor::solana_program::program::invoke(&ix, accounts)
                        .map_err(|_| ::verified_anchor::VAError::InitFailed { field: #fname })?;
                },
            };
            lifecycle_steps.push(quote! {
                {
                    let __needs_init = {
                        let d = accounts[#i].try_borrow_data()
                            .map_err(|_| ::verified_anchor::VAError::InitFailed { field: #fname })?;
                        // This predicate is a deliberate strict SUPERSET of the Lean model's
                        // `isZeroDisc` on the short-data (`len < 8`) case: a genuinely fresh
                        // account may present zero-length data, so we init it (create_account
                        // still establishes the proven owner+size post); it is never an
                        // attacker's already-initialized account.
                        d.len() < 8 || d[0..8] == [0u8; 8]
                    };
                    if __needs_init {
                        // FRESH branch (Lean `applyInit`): create the account owned by this
                        // program and stamp T's REAL discriminator into the first 8 bytes so
                        // the account is a valid, type-correct, non-zero-disc Anchor account.
                        let space_total: usize = #n + 8;
                        let ix = ::verified_anchor::solana_program::system_instruction::create_account(
                            accounts[#pi].key, accounts[#i].key, rent_lamports, space_total as u64, program_id);
                        #init_create
                        let __disc = <#t as ::verified_anchor::AccountData>::DISCRIMINATOR;
                        let mut d = accounts[#i].try_borrow_mut_data()
                            .map_err(|_| ::verified_anchor::VAError::InitFailed { field: #fname })?;
                        if d.len() < 8 {
                            return Err(::verified_anchor::VAError::InitFailed { field: #fname });
                        }
                        d[0..8].copy_from_slice(&__disc);
                    } else {
                        // EXISTING branch (Lean `applyInitIfNeeded` else): the account is
                        // already initialized (non-zero disc). Accept it ONLY if it is a
                        // program-owned account of sufficient size — otherwise this is a
                        // reinit/wrong-account attack and we reject. (`owner` is `&Pubkey`;
                        // `data_len()` is the data length, per the close/realloc steps above.)
                        if accounts[#i].owner != program_id
                            || accounts[#i].data_len() < (#n + 8)
                        {
                            return Err(::verified_anchor::VAError::InitFailed { field: #fname });
                        }
                    }
                }
            });
        }
    }

    quote! {
        pub fn execute_lifecycle(
            accounts: &[::verified_anchor::solana_program::account_info::AccountInfo],
            program_id: &::verified_anchor::solana_program::pubkey::Pubkey,
            rent_lamports: u64,
        ) -> ::core::result::Result<(), ::verified_anchor::VAError> {
            // Bounds guard: the steps index accounts by declared field position. Without this a
            // short slice would panic; the Lean `applyInit`/`applyClose` are none-safe on
            // out-of-range indices, so reject cleanly here to mirror that.
            if accounts.len() < #n {
                return Err(::verified_anchor::VAError::NotEnoughAccounts { expected: #n, got: accounts.len() });
            }
            #(#lifecycle_steps)*
            Ok(())
        }
    }
}

// `instruction` is registered as an INERT helper attribute: `#[instruction(name: String)]` is
// real Anchor source that sits on the struct, and without this rustc rejects it outright
// ("cannot find attribute `instruction` in this scope") before the derive ever runs.
#[proc_macro_derive(VerifiedAccounts, attributes(account, instruction))]
pub fn derive_verified_accounts(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let instr_args = match parse_instruction_args(&input) {
        Ok(a) => a,
        Err(e) => return e.to_compile_error().into(),
    };
    let mut specs = match collect_fields(&input) {
        Ok(s) => s,
        Err(e) => return e.to_compile_error().into(),
    };
    let name = &input.ident;

    // ── Resolve bare seed names ───────────────────────────────────────────────────────────
    //
    // `authority.as_ref()` and `&blob` peel to a bare NAME, and syntax alone cannot say whether
    // that name is a `#[instruction(...)]` argument (a `Pubkey`/`Vec` argument, already
    // byte-shaped) or an account field (meaning its key bytes). Both are real Anchor spellings.
    //
    // THE RULE: consult the declared `#[instruction(...)]` list FIRST, then fall back to the
    // struct's account fields. The argument list is the more specific binding at this position,
    // and a fixed order makes the behaviour predictable rather than dependent on declaration
    // order. Spellings that ARE decidable from syntax never come through here: `.key()` can only
    // be an account, `.as_bytes()`/`.to_le_bytes()` can only be an argument.
    //
    // A name that is BOTH is a compile error, never a guess — silently picking the wrong seed
    // source derives a wrong address, which is the exact failure this whole feature guards
    // against.
    {
        let field_names: Vec<String> = specs.iter().map(|s| s.name.clone()).collect();
        for spec in &mut specs {
            let field = spec.name.clone();
            for c in &mut spec.constraints {
                let Constraint::Seeds(elems) = c else {
                    continue;
                };
                for e in elems {
                    let SeedElem::Unresolved(id) = e else {
                        continue;
                    };
                    let n = id.to_string();
                    let is_arg = instr_args.mappable.iter().any(|a| a.name == n);
                    let is_field = field_names.contains(&n);
                    // CHECKED BEFORE the field fallback, and this order is the whole point: a
                    // DECLARED argument that the unmappable-type cutoff dropped must not quietly
                    // resolve to a same-named account. Anchor evaluates this seed with the
                    // argument in scope, so falling back to the account's key bytes would derive
                    // a DIFFERENT address with no diagnostic — our own tests and Lean model would
                    // both agree with the wrong answer. `#[instruction(params: SomeStruct,
                    // authority: Pubkey)]` next to an `authority` account is ordinary Anchor.
                    if !is_arg && instr_args.declared.contains(&n) {
                        return syn::Error::new_spanned(
                            &*id,
                            format!(
                            "seed `{id}.as_ref()` on field `{field}` names the #[instruction(...)] \
                             argument `{id}`, which verified-anchor had to drop: its type, or the \
                             type of an argument declared before it, is not one verified-anchor \
                             can locate yet, and every argument at or after the first such type \
                             has a Borsh offset that cannot be computed. Give those arguments \
                             mappable types, or move `{id}` ahead of them"),
                        )
                        .to_compile_error()
                        .into();
                    }
                    *e = match (is_arg, is_field) {
                        (true, true) => {
                            return syn::Error::new_spanned(
                                &*id,
                                format!(
                            "seed `{id}.as_ref()` on field `{field}` is ambiguous: `{id}` is both \
                             a declared #[instruction(...)] argument and an account field of this \
                             struct. Rename one of them — guessing would derive a different \
                             address than the one you meant"),
                            )
                            .to_compile_error()
                            .into()
                        }
                        (true, false) => SeedElem::ArgField(id.clone(), ArgSeedForm::Bare),
                        (false, true) => SeedElem::FieldKey(id.clone()),
                        (false, false) => {
                            let args: Vec<&str> = instr_args
                                .mappable
                                .iter()
                                .map(|a| a.name.as_str())
                                .collect();
                            return syn::Error::new_spanned(
                                &*id,
                                format!(
                                "seed `{id}.as_ref()` on field `{field}` names neither a declared \
                                 #[instruction(...)] argument (declared and mappable: [{}]) nor an \
                                 account field of this struct ([{}])",
                                args.join(", "), field_names.join(", ")),
                            )
                            .to_compile_error()
                            .into();
                        }
                    };
                }
            }
        }
    }

    // Guard: every `name.as_bytes()` seed must name a DECLARED, MAPPABLE `#[instruction(..)]`
    // argument. Without this the seed would silently resolve to `None` at runtime and reject
    // every account — a brick, not a bug report. Same precedent as the unlocatable `has_one`
    // target being a build error. Note an argument dropped by the unmappable-type cutoff lands
    // here too, which is correct: its offset is genuinely uncomputable.
    for spec in &specs {
        for c in &spec.constraints {
            let Constraint::Seeds(elems) = c else {
                continue;
            };
            for e in elems {
                let SeedElem::ArgField(id, form) = e else {
                    continue;
                };
                let Some(arg) = instr_args
                    .mappable
                    .iter()
                    .find(|a| a.name == id.to_string())
                else {
                    let mappable: Vec<&str> = instr_args
                        .mappable
                        .iter()
                        .map(|a| a.name.as_str())
                        .collect();
                    let why = if instr_args.declared.contains(&id.to_string()) {
                        // Declared, but at or after the unmappable-type cutoff. Saying "not
                        // declared" here would be actively misleading — the developer DID
                        // declare it and can see it in the attribute.
                        "was dropped: its type, or the type of an argument declared before it, is \
                         not one verified-anchor can locate yet, and every argument at or after \
                         the first such type has a Borsh offset that cannot be computed. Give \
                         those arguments mappable types, or move it ahead of them"
                    } else {
                        "is not declared there; add it to `#[instruction(...)]`"
                    };
                    return syn::Error::new_spanned(
                        id,
                        format!(
                        "seed `{id}.{}` on field `{}` names an #[instruction(...)] argument that \
                         {why} (declared and mappable: [{}])",
                        form.spelling(), spec.name, mappable.join(", ")),
                    )
                    .to_compile_error()
                    .into();
                };
                // The bytes come from the DECLARED TYPE, never from the method name, so a
                // mismatch would silently succeed with bytes the spelling does not describe
                // (`amount.as_bytes()` on a u64 yields the 8-byte LE encoding). Reject it: the
                // seed list must read the way the equivalent Anchor source reads, or the two are
                // only accidentally in agreement.
                let ok = match (form, arg.kind) {
                    (ArgSeedForm::AsBytes, ArgTyKind::Prefixed) => true,
                    (ArgSeedForm::ToLeBytes, ArgTyKind::Numeric) => true,
                    // `Pubkey` and `Vec`/`String` are already byte-shaped, so Anchor reaches them
                    // with a plain `&`/`.as_ref()` rather than a conversion call.
                    (ArgSeedForm::Bare, ArgTyKind::Key | ArgTyKind::Prefixed) => true,
                    _ => false,
                };
                if !ok {
                    let expected = match arg.kind {
                        ArgTyKind::Prefixed => {
                            "`as_bytes()` or `as_ref()` (String/Vec<u8> are length-prefixed)"
                        }
                        ArgTyKind::Numeric => "`to_le_bytes()` (Borsh integers are little-endian)",
                        ArgTyKind::Key => "`as_ref()` (a Pubkey is already byte-shaped)",
                        ArgTyKind::Other => {
                            "no seed spelling yet — seeds can be String/Vec<u8> (`as_bytes()` \
                             or `as_ref()`), an integer (`to_le_bytes()`) or a Pubkey \
                             (`as_ref()`). Note a `Vec<T>` with a wider element is NOT \
                             byte-shaped: its 4-byte prefix counts ELEMENTS, not bytes"
                        }
                    };
                    return syn::Error::new_spanned(
                        id,
                        format!(
                            "seed `{id}.{}` on field `{}` does not match the declared type of \
                         `{id}` in #[instruction(...)]; expected {expected}",
                            form.spelling(),
                            spec.name
                        ),
                    )
                    .to_compile_error()
                    .into();
                }
            }
        }
    }

    // Guard: a `<field>.key()` seed must name a field of THIS struct. Raised here, with a span,
    // rather than left to the `index_of` lookups inside `validate_body`/the `Bumps` init: those
    // are `panic!`s, and a proc-macro panic surfaces as `proc-macro derive panicked` with no
    // source location at all. Two lookup sites share this one guard.
    for spec in &specs {
        for c in &spec.constraints {
            let Constraint::Seeds(elems) = c else {
                continue;
            };
            for e in elems {
                let SeedElem::FieldKey(id) = e else { continue };
                if !specs.iter().any(|s| s.name == id.to_string()) {
                    let fields: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
                    return syn::Error::new_spanned(
                        id,
                        format!(
                            "seed `{id}.key()` on field `{}` is not a field of this struct \
                         (fields: [{}])",
                            spec.name,
                            fields.join(", ")
                        ),
                    )
                    .to_compile_error()
                    .into();
                }
            }
        }
    }

    // Guard: `init_if_needed` cannot combine with an instruction-data seed. `execute_lifecycle`
    // has no `instr_data` to derive the PDA from, so the signer seeds cannot be rebuilt there.
    // This is VALID Anchor source, so it must fail as a spanned compile error explaining the
    // limitation — not as the `panic!` inside `lifecycle_body`, which loses the span entirely.
    for spec in &specs {
        if !spec
            .constraints
            .iter()
            .any(|c| matches!(c, Constraint::InitIfNeeded))
        {
            continue;
        }
        for c in &spec.constraints {
            let Constraint::Seeds(elems) = c else {
                continue;
            };
            let bad = elems.iter().find_map(|e| match e {
                SeedElem::ArgField(id, form) => Some(format!("{id}.{}", form.spelling())),
                SeedElem::InstrArg(off, len) => Some(format!("arg({off}, {len})")),
                _ => None,
            });
            if let Some(bad) = bad {
                return syn::Error::new_spanned(
                    name,
                    format!(
                    "field `{}` combines `init_if_needed` with the instruction-data seed `{bad}`, \
                     which verified-anchor cannot support: the account is created in \
                     `execute_lifecycle`, which receives no instruction data and so cannot \
                     rebuild the PDA's signer seeds. Use literal or `<field>.key()` seeds, or \
                     create the account with an explicit `init`",
                    spec.name),
                )
                .to_compile_error()
                .into();
            }
        }
    }

    // Guard: `realloc` requires `mut` — realloc mutates the account data in place.
    // Guard: `init_if_needed` requires a typed `Account<'info, T>` — the wrapper's
    //        owner+discriminator checks are the reinit guard (prevents reuse attacks).
    for spec in &specs {
        let has_realloc = spec
            .constraints
            .iter()
            .any(|c| matches!(c, Constraint::Realloc(_)));
        let has_mut = spec
            .constraints
            .iter()
            .any(|c| matches!(c, Constraint::Mut));
        if has_realloc && !has_mut {
            return syn::Error::new_spanned(
                name,
                format!(
                    "field `{}` uses `realloc` but is not `mut`; realloc mutates the account",
                    spec.name
                ),
            )
            .to_compile_error()
            .into();
        }
        if spec
            .constraints
            .iter()
            .any(|c| matches!(c, Constraint::InitIfNeeded))
            && !matches!(spec.kind, WrapperKind::Account(_))
        {
            return syn::Error::new_spanned(name,
                format!("field `{}` uses `init_if_needed` but is not a typed `Account<'info, T>`; the wrapper's owner+discriminator checks are the reinit guard", spec.name))
                .to_compile_error().into();
        }
        // Guard: `has_one` requires a typed `Account<'info, T>` — the target field's Borsh
        // offset comes from `T::LAYOUT`, and an untyped wrapper has none. Raised HERE rather
        // than only inside `validate_body` so the user gets one clean error instead of a
        // cascade from the missing `Validate::validate`. (`has_one` on an `UncheckedAccount`
        // is not valid stock Anchor either, and Lean's `AccountType.locateField` returns
        // `none` for non-account wrappers, so the model cannot express it.)
        if spec
            .constraints
            .iter()
            .any(|c| matches!(c, Constraint::HasOne(_)))
            && !matches!(spec.kind, WrapperKind::Account(_))
        {
            return syn::Error::new_spanned(name,
                format!("field `{}` uses `has_one` but is not a typed `Account<'info, T>`; the target's Borsh offset comes from `T::LAYOUT`", spec.name))
                .to_compile_error().into();
        }
    }

    let body = validate_body(&specs, &instr_args.mappable);
    let (lean_tpl, lean_args) = lean_spec_string(&specs, &instr_args.mappable);

    // ── The escape hatch (M10 Task 13) ────────────────────────────────────────────────────
    //
    // Every `constraint = <expr>` the proven core does not cover runs here instead: verbatim
    // Rust, in `try_accounts`, AFTER deserialisation, so `vault.amount` means what Anchor
    // means. It cannot run in `validate` — that is the byte-level path and the deserialised
    // bindings do not exist yet. Both paths are conjuncts of the same `try_accounts`, so an
    // unproven check can only ever REJECT MORE than the proof describes: soundness
    // ("verified-anchor never accepts an account set the contract rejects") is untouched, only
    // completeness is.
    let (hatch_index_of, hatch_inner_ty) = expr_maps(&specs);
    let hatch_arg_names: Vec<String> = instr_args.mappable.iter().map(|a| a.name.clone()).collect();
    let hatch_ctx = crate::expr::ExprCtx {
        index_of: &hatch_index_of,
        inner_ty: &hatch_inner_ty,
        instr_args: &hatch_arg_names,
        deser: false,
    };
    let unproven = unproven_checks(&specs, &hatch_ctx, &instr_args.mappable);
    let field_names: Vec<String> = specs.iter().map(|s| s.name.clone()).collect();

    // `UNPROVEN_CHECKS` entries. A conditionally proven expression contributes `""` in the
    // builds where the proof does cover it; the empties are filtered out below so the reported
    // list is exactly the set of checks running outside the proof in THIS build.
    let unproven_srcs: Vec<TokenStream2> = unproven
        .iter()
        .map(|u| {
            let src = &u.src;
            match &u.proven_if {
                Some((cond, _)) => quote! { if #cond { "" } else { #src } },
                None => quote! { #src },
            }
        })
        .collect();

    let hatch_ctx_deser = crate::expr::ExprCtx {
        index_of: &hatch_index_of,
        inner_ty: &hatch_inner_ty,
        instr_args: &hatch_arg_names,
        deser: true,
    };
    // The developer's expression run VERBATIM as Rust, as a self-contained block. Used both for
    // expressions outside the sublanguage and as the const-selected fallback for a sublanguage
    // expression the readability gate turned off (see `UnprovenCheck::proven_if`).
    let verbatim_check = |field: &str, src: &str, e: &Expr| -> TokenStream2 {
        let used = idents_in(e);
        // Bind every account field the expression names, by its DECLARED name, so the
        // developer's Rust reads exactly what the same source reads under real Anchor:
        // `Account<'info, T>` derefs to the deserialised `T`, the untyped wrappers
        // deref to `AccountInfo`. Only the names actually used are bound — an unused
        // binding would warn in the user's crate, which they cannot silence.
        let binds: Vec<TokenStream2> = specs
            .iter()
            .filter(|sp| used.contains(&sp.name))
            .map(|sp| {
                let id = syn::Ident::new(&sp.name, name.span());
                // Same reasoning as the argument bindings below: `idents_in` may over-report
                // (an account field name reached only through a non-value position), and the
                // resulting warning would be unsilenceable in the user's crate.
                quote! { #[allow(unused_variables)] let #id = &__self.#id; }
            })
            .collect();
        let arg_binds = instr_arg_binds(
            &instr_args.all,
            &used,
            &field_names,
            field,
            src,
            name.span(),
        );
        quote! {
            {
                #(#binds)*
                #arg_binds
                if !(#e) {
                    return ::core::result::Result::Err(
                        ::verified_anchor::VAError::ConstraintViolated {
                            field: #field, expr: #src });
                }
            }
        }
    };

    let hatch_checks: Vec<TokenStream2> = unproven
        .iter()
        .map(|u| {
            let (field, src, e) = (&u.field, &u.src, u.expr);
            match &u.proven_if {
                // Const-selected fallback for a SUBLANGUAGE expression whose data fields the user's
                // descriptor cannot read at the byte level. The proven check in `validate` is
                // switched off by the SAME const, so exactly one of the two runs — never neither.
                Some((cond, Some(v))) => {
                    let check = v.to_tokens_check(field, src, &hatch_ctx_deser);
                    // `Operand::instrArg` reads `instr_data` through this const, which
                    // `try_accounts` only emits for seeds. Shadowing it locally is harmless and
                    // keeps the fallback self-contained.
                    let args_decl = match v.uses_instr_arg() {
                        true => instr_args_const(&instr_args.mappable),
                        false => quote! {},
                    };
                    quote! { if !(#cond) { #args_decl #check } }
                }
                // Same const selection, but the fallback is the developer's verbatim Rust — the only
                // form that can read an AGGREGATE field (`[T; N]`, `String`, `Vec<T>`, `Option<T>`),
                // which both `read_val` and `layout::FieldValue` refuse.
                Some((cond, None)) => {
                    let body = verbatim_check(field, src, e);
                    quote! { if !(#cond) #body }
                }
                // Outside the sublanguage: the developer's Rust, verbatim, in every build.
                None => verbatim_check(field, src, e),
            }
        })
        .collect();

    let lifecycle = lifecycle_body(&specs);
    let has_lifecycle = specs.iter().any(|s| {
        s.constraints.iter().any(|c| {
            matches!(
                c,
                Constraint::InitMarker
                    | Constraint::Close(_)
                    | Constraint::Realloc(_)
                    | Constraint::InitIfNeeded
            )
        })
    });
    let name_str = name.to_string();

    let has_info = !specs.is_empty();
    let bumps_struct_name = syn::Ident::new(&format!("{}Bumps", name), name.span());

    // Identify seeded fields (those with a Constraint::Seeds), preserving order.
    let seeded: Vec<(usize, &FieldSpec, &Vec<SeedElem>)> = specs
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            s.constraints.iter().find_map(|c| {
                if let Constraint::Seeds(elems) = c {
                    Some((i, s, elems))
                } else {
                    None
                }
            })
        })
        .collect();

    // Build name→index map for resolving `field.key()` seeds in Bumps init.
    let bumps_index_of: std::collections::HashMap<String, usize> = specs
        .iter()
        .enumerate()
        .map(|(i, s)| (s.name.clone(), i))
        .collect();

    // Per-seeded-field: (Bumps-field Ident, seed slice exprs, derivation program-id token).
    // The `seeds::program` override applies here too so the canonical bump exposed in `Bumps`
    // is derived against the SAME foreign program id used by `validate`.
    let bumps_fields: Vec<(syn::Ident, Vec<TokenStream2>, TokenStream2)> = seeded
        .iter()
        .map(|(_, spec, elems)| {
            let fname = syn::Ident::new(&spec.name, name.span());
            let bumps_fname: &str = &spec.name;
            let seed_exprs: Vec<TokenStream2> = elems.iter().map(|se| match se {
            SeedElem::Literal(b) => quote! { &#b[..] },
            SeedElem::FieldKey(id) => {
                // Guarded with a span in `derive_verified_accounts`, which runs first.
                let fi = *bumps_index_of.get(&id.to_string())
                    .unwrap_or_else(|| unreachable!("seed field `{id}` reached codegen"));
                quote! { accounts[#fi].key.as_ref() }
            }
            SeedElem::InstrArg(off, len) => {
                let end = off + len;
                // Clamp to length (matches the validate-side seed slice and the Lean model).
                quote! { &instr_data[(#off).min(instr_data.len())..(#end).min(instr_data.len())] }
            }
            // Same `argBytes` mirror as `validate`. Unreachable in practice (validate already
            // rejected a seed it could not resolve) but still TOTAL: `try_accounts` returns
            // `Result<_, VAError>`, so the fail-closed `?`/`return` inside this expression
            // type-checks here exactly as it does in `validate`.
            SeedElem::ArgField(id, _) => arg_field_seed_expr(id, bumps_fname),
            SeedElem::Unresolved(id) => unreachable!("unresolved seed `{id}` reached codegen"),
        }).collect();
            let derive_pid: TokenStream2 = match spec.constraints.iter().find_map(|c| match c {
                Constraint::SeedsProgram(e) => Some(e),
                _ => None,
            }) {
                Some(expr) => quote! { &(#expr) },
                None => quote! { program_id },
            };
            (fname, seed_exprs, derive_pid)
        })
        .collect();

    let (bumps_struct_decl, bumps_struct_init) = if bumps_fields.is_empty() {
        (
            quote! { pub struct #bumps_struct_name; },
            quote! { #bumps_struct_name },
        )
    } else {
        let decl_fields: Vec<TokenStream2> = bumps_fields
            .iter()
            .map(|(fname, _, _)| {
                quote! { pub #fname: u8 }
            })
            .collect();
        let init_fields: Vec<TokenStream2> = bumps_fields.iter().map(|(fname, seed_exprs, derive_pid)| {
            quote! {
                #fname: {
                    let __seeds: &[&[u8]] = &[ #(#seed_exprs),* ];
                    let (_pda, __b) = ::verified_anchor::solana_program::pubkey::Pubkey::find_program_address(__seeds, #derive_pid);
                    __b
                }
            }
        }).collect();
        (
            quote! { pub struct #bumps_struct_name { #(#decl_fields),* } },
            quote! { #bumps_struct_name { #(#init_fields),* } },
        )
    };

    let field_inits: Vec<TokenStream2> = specs.iter().enumerate().map(|(i, spec)| {
        let fname = syn::Ident::new(&spec.name, name.span());
        match &spec.kind {
            WrapperKind::Account(t) => quote! {
                #fname: {
                    let raw = accounts[#i].data.borrow();
                    let bytes = raw.get(8..).ok_or(::verified_anchor::VAError::BorshFailed { field: stringify!(#fname) })?.to_vec();
                    drop(raw);
                    ::verified_anchor::Account {
                        info: &accounts[#i],
                        data: <#t as ::verified_anchor::borsh::BorshDeserialize>::try_from_slice(&bytes)
                            .map_err(|_| ::verified_anchor::VAError::BorshFailed { field: stringify!(#fname) })?,
                    }
                }
            },
            WrapperKind::Signer => quote! {
                #fname: ::verified_anchor::Signer { info: &accounts[#i] }
            },
            WrapperKind::Program(p) => quote! {
                #fname: ::verified_anchor::Program::<'info, #p>::new(&accounts[#i])
            },
            WrapperKind::SystemAccount => quote! {
                #fname: ::verified_anchor::SystemAccount { info: &accounts[#i] }
            },
            WrapperKind::Unchecked => quote! {
                #fname: ::verified_anchor::UncheckedAccount { info: &accounts[#i] }
            },
        }
    }).collect();

    let validate_impl = if has_info {
        quote! { impl<'info> ::verified_anchor::Validate for #name<'info> { #body } }
    } else {
        quote! { impl ::verified_anchor::Validate for #name { #body } }
    };
    let accounts_impl_target = if has_info {
        quote! { #name<'info> }
    } else {
        quote! { #name }
    };
    let lean_spec_impl_target = if has_info {
        quote! { #name<'_> }
    } else {
        quote! { #name }
    };
    // `UNPROVEN_CHECKS`: the source text of every check running OUTSIDE the proof, reported by
    // `cargo verified-anchor check`. Host-only, like `lean_spec`: it is a development-time
    // report, and there is no reason to carry the strings in the on-chain `.so`.
    let any_conditional = unproven.iter().any(|u| u.proven_if.is_some());
    let unproven_all = syn::Ident::new(&format!("__VA_UNPROVEN_ALL_{}", name), name.span());
    let unproven_len = syn::Ident::new(&format!("__VA_UNPROVEN_LEN_{}", name), name.span());
    let unproven_arr = syn::Ident::new(&format!("__VA_UNPROVEN_{}", name), name.span());
    // A conditionally proven expression contributes `""` in builds where the proof covers it.
    // Those blanks are filtered out AT COMPILE TIME so the reported list has no holes; the
    // length is a const, so it has to be counted in a const too.
    let (unproven_items, unproven_slice) = if any_conditional {
        (
            quote! {
                #[cfg(not(target_os = "solana"))]
                #[doc(hidden)]
                #[allow(non_upper_case_globals)]
                const #unproven_all: &[&str] = &[#(#unproven_srcs),*];
                #[cfg(not(target_os = "solana"))]
                #[doc(hidden)]
                #[allow(non_upper_case_globals)]
                const #unproven_len: usize = {
                    let mut n = 0usize;
                    let mut i = 0usize;
                    while i < #unproven_all.len() {
                        if !#unproven_all[i].is_empty() { n += 1; }
                        i += 1;
                    }
                    n
                };
                #[cfg(not(target_os = "solana"))]
                #[doc(hidden)]
                #[allow(non_upper_case_globals)]
                const #unproven_arr: [&str; #unproven_len] = {
                    let mut out = [""; #unproven_len];
                    let mut i = 0usize;
                    let mut j = 0usize;
                    while i < #unproven_all.len() {
                        if !#unproven_all[i].is_empty() {
                            out[j] = #unproven_all[i];
                            j += 1;
                        }
                        i += 1;
                    }
                    out
                };
            },
            quote! { &#unproven_arr },
        )
    } else {
        (quote! {}, quote! { &[#(#unproven_srcs),*] })
    };

    // The Bumps init inside `try_accounts` re-derives seeds, so it needs the same const.
    let try_accounts_instr_args = if uses_arg_field(&specs) {
        instr_args_const(&instr_args.mappable)
    } else {
        quote! {}
    };

    let expanded = quote! {
        #unproven_items
        #validate_impl
        // See the note in `account_data_derive`: `target_os = "solana"` is not a known
        // check-cfg value, so the derive must silence the warning on its users' behalf.
        // `target_os = "solana"` is not in rustc's built-in check-cfg list, so the `#[cfg]`
        // below would otherwise raise an `unexpected_cfgs` warning in every user crate — one
        // the user cannot silence, since it originates in this expansion. (Verified: the same
        // attribute on the `inventory::submit!` item below has no effect, because attributes
        // do not propagate into a macro-invocation item; that pre-existing warning stands.)
        #[allow(unexpected_cfgs)]
        impl #lean_spec_impl_target {
            /// The `AccountsStruct` literal for this struct (Lean source), built at CALL time
            /// so typed fields can splice in their real Borsh layout.
            ///
            /// Host-only, like the `inventory` registration below: the spliced
            /// `AccountData::LAYOUT_LEAN` is itself host-only, and there is no reason to pay
            /// for the Lean source string inside the on-chain `.so`.
            #[cfg(not(target_os = "solana"))]
            pub fn lean_spec() -> ::std::string::String {
                ::std::format!(#lean_tpl #(, #lean_args)*)
            }
            /// Source text of every `constraint = <expr>` that runs OUTSIDE the proof — as
            /// raw Rust in `try_accounts`, after deserialisation, with Anchor's semantics
            /// rather than `evalExpr`'s. Empty for a fully proven struct.
            ///
            /// These checks still RUN, and being extra `&&` conjuncts they can only reject
            /// more than the proof describes; what they are not is modelled in Lean.
            #[cfg(not(target_os = "solana"))]
            pub const UNPROVEN_CHECKS: &'static [&'static str] = #unproven_slice;
            #lifecycle
        }
        #bumps_struct_decl
        impl<'info> ::verified_anchor::Accounts<'info> for #accounts_impl_target {
            type Bumps = #bumps_struct_name;
            fn try_accounts(
                program_id: &::verified_anchor::solana_program::pubkey::Pubkey,
                accounts: &'info [::verified_anchor::solana_program::account_info::AccountInfo<'info>],
                instr_data: &[u8],
            ) -> ::core::result::Result<(Self, Self::Bumps), ::verified_anchor::VAError> {
                #try_accounts_instr_args
                <Self as ::verified_anchor::Validate>::validate(accounts, instr_data, program_id)?;
                let __self = Self { #(#field_inits),* };
                // The escape hatch: checks the proof does not cover, run verbatim against the
                // DESERIALISED bindings. After `validate` and after `__self`, because that is
                // the only point where `vault.amount` can mean what Anchor means.
                #(#hatch_checks)*
                let __bumps = #bumps_struct_init;
                ::core::result::Result::Ok((__self, __bumps))
            }
        }
        // Host-only: `inventory` corrupts the Solana SBF ELF, so this registration must NOT
        // be compiled into a BPF program. Gated by target_os, matching verified-anchor's lib.
        #[cfg(not(target_os = "solana"))]
        ::verified_anchor::inventory::submit! {
            ::verified_anchor::SpecEntry {
                name: #name_str,
                lean_spec: <#lean_spec_impl_target>::lean_spec,
                has_lifecycle: #has_lifecycle,
                unproven: <#lean_spec_impl_target>::UNPROVEN_CHECKS,
            }
        }
    };
    expanded.into()
}
