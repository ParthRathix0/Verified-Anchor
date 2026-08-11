use proc_macro::TokenStream;

mod account_data_derive;
mod account_attr;
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
    Literal(syn::LitByteStr),   // b"vault"
    FieldKey(syn::Ident),       // field.key()
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
        let last = path.segments.last().ok_or_else(||
            syn::Error::new_spanned(ty, "verified-anchor: unrecognised field type"))?;
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
        Err(syn::Error::new_spanned(ty, "verified-anchor: unrecognised field type"))
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
        WrapperKind::SystemAccount => vec![
            Constraint::Owner(syn::parse_quote! { ::verified_anchor::solana_program::system_program::ID }),
        ],
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
                        return Err(syn::Error::new(key.span(),
                            "unsupported `seeds::` key (expected `seeds::program = <expr>`)"));
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
                        Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(i), .. }) => {
                            Ok(Constraint::BumpDeclared(i.base10_parse()?))
                        }
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
                        other => Err(syn::Error::new_spanned(other,
                            "unsupported `bump = <expr>` (expected a u8 literal or `arg(off)`)")),
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
                    other => Err(syn::Error::new(mode.span(),
                        format!("expected `enforce` or `skip` after `rent_exempt =`, got `{other}`"))),
                }
            }
            "zero" => Ok(Constraint::Zero),
            "init_if_needed" => Ok(Constraint::InitIfNeeded),
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
                    "constraint", "token", "mint",
                    "associated_token", "owner_program",
                    "token_program",
                ];
                let hint = if known_unsupported.contains(&other) {
                    format!("`{other}` is a stock-Anchor constraint that verified-anchor does not support")
                } else {
                    format!("unknown constraint `{other}`")
                };
                Err(syn::Error::new(
                    ident.span(),
                    format!("{hint}; verified-anchor supports: signer, mut, owner, has_one, allow_duplicate, init, init_if_needed, payer, space, close, seeds, seeds::program, bump, discriminator, address, executable, rent_exempt, realloc, realloc::payer, realloc::zero, zero. See docs/migrating-from-anchor.md"),
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
        Expr::Path(p) => p.path.get_ident().map(|id| SeedElem::Unresolved(id.clone())),
        // The peeling applies to LITERAL seeds too: `b"vault".as_ref()` and `&b"vault"` are the
        // same seed as the bare `b"vault"` that `parse_seed_elem` matches first.
        Expr::Lit(syn::ExprLit { lit: syn::Lit::ByteStr(b), .. }) => Some(SeedElem::Literal(b.clone())),
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
                    Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(sl), .. }) =>
                        Some(SeedElem::Literal(syn::LitByteStr::new(sl.value().as_bytes(), sl.span()))),
                    _ => recv_ident().map(|id| SeedElem::ArgField(id, ArgSeedForm::AsBytes)),
                },
                "to_le_bytes" => recv_ident().map(|id| SeedElem::ArgField(id, ArgSeedForm::ToLeBytes)),
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
        Expr::Lit(syn::ExprLit { lit: syn::Lit::ByteStr(b), .. }) => Ok(SeedElem::Literal(b)),
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
                return Err(syn::Error::new_spanned(&e, format!(
                    "seed `{bad}` is not supported: Borsh — and therefore the PDA Anchor derives \
                     — is LITTLE-endian, so this would silently derive a different address than \
                     the same program under Anchor. Use `to_le_bytes()`.")));
            }
            match peel_seed(&e) {
                Some(se) => Ok(se),
                None => Err(syn::Error::new_spanned(&e,
                    "unsupported seed (expected b\"..\", field.key(), name.as_bytes(), \
                     amount.to_le_bytes(), a Pubkey/Vec argument via `.as_ref()`, \
                     or arg(off, len))")),
            }
        }
        Expr::Call(call) => {
            let is_arg = matches!(call.func.as_ref(),
                Expr::Path(p) if p.path.is_ident("arg"));
            if !is_arg {
                return Err(syn::Error::new_spanned(call.func, "unsupported seed call (expected `arg(off, len)`)"));
            }
            let mut it = call.args.iter();
            let off = lit_usize(it.next())?;
            let len = lit_usize(it.next())?;
            Ok(SeedElem::InstrArg(off, len))
        }
        other => Err(syn::Error::new_spanned(other,
            "unsupported seed (expected b\"..\", field.key(), name.as_bytes(), or arg(off, len))")),
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
        "u8" | "u16" | "u32" | "u64" | "u128"
        | "i8" | "i16" | "i32" | "i64" | "i128" => ArgTyKind::Numeric,
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
}

/// Is this `Vec<...>` segment's element type exactly `u8`?
fn vec_elem_is_u8(seg: &syn::PathSegment) -> bool {
    let syn::PathArguments::AngleBracketed(a) = &seg.arguments else { return false };
    a.args.iter().any(|g| matches!(g,
        syn::GenericArgument::Type(syn::Type::Path(p)) if p.path.is_ident("u8")))
}

/// Parse `#[instruction(amount: u64, name: String)]` off the derive input.
///
/// An argument whose type is not mappable STOPS the mappable list: every argument after it sits
/// at an offset we cannot compute (Borsh is positional and variable-width), so silently keeping
/// it would hand later arguments a wrong offset. This is the same cutoff rule the account-layout
/// derive uses. The names are still recorded in `declared`, so a seed naming one is a compile
/// error rather than a runtime brick OR a silent fallback to a same-named account.
fn parse_instruction_args(input: &DeriveInput) -> syn::Result<InstrArgs> {
    let attr = match input.attrs.iter().find(|a| a.path().is_ident("instruction")) {
        Some(a) => a,
        None => return Ok(InstrArgs { mappable: Vec::new(), declared: Vec::new() }),
    };
    let parser = Punctuated::<syn::PatType, Token![,]>::parse_terminated;
    let parsed = attr.parse_args_with(parser)?;
    let mut mappable = Vec::new();
    let mut declared = Vec::new();
    let mut past_cutoff = false;
    for pt in parsed {
        let name = match pt.pat.as_ref() {
            syn::Pat::Ident(i) => i.ident.to_string(),
            other => return Err(syn::Error::new_spanned(other, "expected an argument name")),
        };
        declared.push(name.clone());
        // Keep walking after the cutoff purely to finish recording names — nothing past it can
        // be mapped, because its offset depends on the width we could not compute.
        if past_cutoff {
            continue;
        }
        match crate::ty_map::map_ty(&pt.ty) {
            Some((rt, lean)) =>
                mappable.push(InstrArg { name, rt, lean, kind: classify_arg_ty(&pt.ty) }),
            None => past_cutoff = true,
        }
    }
    Ok(InstrArgs { mappable, declared })
}

/// Does any field resolve a `name.as_bytes()` seed? Gates emission of the `INSTR_ARGS` const.
fn uses_arg_field(specs: &[FieldSpec]) -> bool {
    specs.iter().any(|s| s.constraints.iter().any(|c| matches!(c,
        Constraint::Seeds(elems) if elems.iter().any(|e| matches!(e, SeedElem::ArgField(_, _))))))
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
        return Err(syn::Error::new_spanned(input, "VerifiedAccounts requires a struct"));
    };
    let Fields::Named(named) = &ds.fields else {
        return Err(syn::Error::new_spanned(&ds.fields, "VerifiedAccounts requires named fields"));
    };
    let mut specs = Vec::new();
    for field in &named.named {
        let name = field.ident.as_ref().unwrap().to_string();
        let mut constraints = Vec::new();
        for attr in &field.attrs {
            if attr.path().is_ident("account") {
                let parsed = attr.parse_args_with(
                    Punctuated::<Constraint, Token![,]>::parse_terminated,
                )?;
                constraints.extend(parsed);
            }
        }
        let kind = classify_field_type(&field.ty)?;
        specs.push(FieldSpec { name, constraints, kind });
    }
    Ok(specs)
}

fn lean_constraint(c: &Constraint) -> String {
    match c {
        Constraint::Signer => "Constraint.signer".to_string(),
        Constraint::Mut => "Constraint.mut".to_string(),
        Constraint::Owner(_) => "Constraint.owner ownerPlaceholder".to_string(),
        Constraint::HasOne(t) => format!("Constraint.hasOne \"{}\"", t),
        // Lifecycle markers: not validation constraints; skip in lean_spec output.
        Constraint::InitMarker | Constraint::Payer(_) | Constraint::Space(_) | Constraint::Close(_) => String::new(),
        Constraint::Seeds(elems) => {
            let seeds: Vec<String> = elems.iter().map(|se| match se {
                SeedElem::Literal(b) => {
                    let bytes: Vec<String> = b.value().iter().map(|x| x.to_string()).collect();
                    format!("SeedSpec.literal (ByteArray.mk #[{}])", bytes.join(", "))
                }
                SeedElem::FieldKey(id) => format!("SeedSpec.fieldKey \"{}\"", id),
                SeedElem::InstrArg(off, len) => format!("SeedSpec.instrArg {} {}", off, len),
                SeedElem::ArgField(id, _) => format!("SeedSpec.argField \"{}\"", id),
                SeedElem::Unresolved(id) => unreachable!("unresolved seed `{id}` reached codegen"),
            }).collect();
            format!("Constraint.seeds [{}] @@BUMP@@ @@PROG@@", seeds.join(", "))
        }
        // The program override is assembled into the seeds spec's third field below; emit nothing
        // standalone (same pattern as bumps).
        Constraint::SeedsProgram(_) => String::new(),
        Constraint::BumpCanonical | Constraint::BumpDeclared(_) | Constraint::BumpStored(_) => String::new(),
        Constraint::Discriminator(d) => {
            let bytes: Vec<String> = d.iter().map(|x| x.to_string()).collect();
            format!("Constraint.discriminator (ByteArray.mk #[{}])", bytes.join(", "))
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
        Constraint::Realloc(_) | Constraint::ReallocPayer(_) | Constraint::ReallocZero(_)
        | Constraint::InitIfNeeded => String::new(),
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
    for spec in specs {
        let cs: Vec<String> = spec.constraints.iter()
            .map(lean_constraint)
            .filter(|s| !s.is_empty())
            .collect();
        let mut cs = cs;   // make mutable
        // init: assemble InitMarker + Payer + Space -> Constraint.init "<payer>" <space> Pubkey.zero
        if spec.constraints.iter().any(|c| matches!(c, Constraint::InitMarker)) {
            let payer = spec.constraints.iter().find_map(|c|
                if let Constraint::Payer(p) = c { Some(p.to_string()) } else { None });
            let space = spec.constraints.iter().find_map(|c|
                if let Constraint::Space(n) = c { Some(*n) } else { None });
            if let (Some(payer), Some(space)) = (payer, space) {
                cs.push(format!("Constraint.init \"{}\" {} Pubkey.zero", payer, space));
            }
        }
        // close: Close(dest) -> Constraint.close "<dest>"
        if let Some(dest) = spec.constraints.iter().find_map(|c|
            if let Constraint::Close(d) = c { Some(d.to_string()) } else { None }) {
            cs.push(format!("Constraint.close \"{}\"", dest));
        }
        // realloc: Realloc(newLen) + ReallocPayer(p) [+ ReallocZero(z)] -> Constraint.realloc "<p>" <newLen> <z>
        if let Some(newlen) = spec.constraints.iter().find_map(|c|
            if let Constraint::Realloc(n) = c { Some(*n) } else { None }) {
            let payer = spec.constraints.iter().find_map(|c|
                if let Constraint::ReallocPayer(p) = c { Some(p.to_string()) } else { None });
            let zero = spec.constraints.iter().any(|c| matches!(c, Constraint::ReallocZero(true)));
            if let Some(payer) = payer {
                cs.push(format!("Constraint.realloc \"{}\" {} {}", payer, newlen, zero));
            }
        }
        // init_if_needed: InitIfNeeded + Payer + Space -> Constraint.initIfNeeded "<payer>" <space> Pubkey.zero
        if spec.constraints.iter().any(|c| matches!(c, Constraint::InitIfNeeded)) {
            let payer = spec.constraints.iter().find_map(|c|
                if let Constraint::Payer(p) = c { Some(p.to_string()) } else { None });
            let space = spec.constraints.iter().find_map(|c|
                if let Constraint::Space(n) = c { Some(*n) } else { None });
            if let (Some(payer), Some(space)) = (payer, space) {
                cs.push(format!("Constraint.initIfNeeded \"{}\" {} Pubkey.zero", payer, space));
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
                format!("AccountType.account \"@@ARG{name_hole}@@\" @@ARG{layout_hole}@@ Pubkey.zero")
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
        let bump_str = spec.constraints.iter().find_map(|c| match c {
            Constraint::BumpCanonical => Some("BumpSpec.canonical".to_string()),
            Constraint::BumpDeclared(d) => Some(format!("(BumpSpec.declared {})", d)),
            Constraint::BumpStored(off) => Some(format!("(BumpSpec.stored {})", off)),
            _ => None,
        }).unwrap_or_else(|| "BumpSpec.canonical".to_string());
        // `seeds::program` override → the third `Constraint.seeds` field. Present ⇒ the schematic
        // placeholder `(some Pubkey.zero)` (the soundness theorem is ∀ over the pubkey, exactly
        // like `owner`/`address`); absent ⇒ `none` (derive against this program's id).
        let prog_str = if spec.constraints.iter().any(|c| matches!(c, Constraint::SeedsProgram(_))) {
            "(some Pubkey.zero)"
        } else {
            "none"
        };
        let cs_joined = cs.join(", ")
            .replace("@@BUMP@@", &bump_str)
            .replace("@@PROG@@", prog_str);
        // `allow_duplicate = <field>` opt-outs → the field's `allowDuplicate` list. Emitted
        // ONLY when non-empty so existing literals keep relying on the Lean field default `[]`.
        let allows: Vec<String> = spec.constraints.iter().filter_map(|c| match c {
            Constraint::AllowDuplicate(t) => Some(format!("\"{}\"", t)),
            _ => None,
        }).collect();
        let allow_str = if allows.is_empty() {
            String::new()
        } else {
            format!(", allowDuplicate := [{}]", allows.join(", "))
        };
        fields.push(format!(
            "{{ name := \"{}\", ty := {}, constraints := [{}]{} }}",
            spec.name,
            ty,
            cs_joined,
            allow_str
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
            instr_args.iter()
                .map(|a| format!("(\"{}\", {})", a.name, a.lean))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let raw = format!("{{ programId := Pubkey.zero{}, fields :={} }}", instr_args_str, body);
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
    // Build a name→index map so has_one can look up the target field's position.
    let index_of: std::collections::HashMap<String, usize> =
        specs.iter().enumerate().map(|(i, s)| (s.name.clone(), i)).collect();
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
        let has_iin = spec.constraints.iter().any(|c| matches!(c, Constraint::InitIfNeeded));
        let effective: Vec<Constraint> = implied.into_iter()
            .filter(|c| !(has_iin && matches!(c, Constraint::Owner(_) | Constraint::Discriminator(_))))
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
                    let tidx = *index_of.get(&tname)
                        .unwrap_or_else(|| panic!("has_one target `{tname}` is not a field of this struct"));
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
                            // map (fixed-size arrays, nested structs, enums are not covered yet),
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
                                    "` has a type verified-anchor cannot map yet (fixed-size arrays, nested \
                                     structs, enums): the layout is truncated at the first such field, because \
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
                },
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
                },
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
                },
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
                Constraint::InitMarker | Constraint::Payer(_) | Constraint::Space(_) | Constraint::Close(_) => {
                    continue;
                }
                // Seeds/bump/seeds::program are handled in the per-field PDA block below.
                Constraint::Seeds(_) | Constraint::SeedsProgram(_) | Constraint::BumpCanonical
                | Constraint::BumpDeclared(_) | Constraint::BumpStored(_) => {
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
                Constraint::Realloc(_) | Constraint::ReallocPayer(_) | Constraint::ReallocZero(_)
                | Constraint::InitIfNeeded => {
                    continue;
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
        if let Some(Constraint::Seeds(elems)) = spec.constraints.iter()
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
        wrapper_implied(&spec.kind).iter().chain(spec.constraints.iter())
            .any(|c| matches!(c, Constraint::Mut))
    };
    let allows = |spec: &FieldSpec, other: &str| -> bool {
        spec.constraints.iter().any(|c|
            matches!(c, Constraint::AllowDuplicate(t) if t == other))
    };
    let mut_indices: Vec<usize> = specs.iter().enumerate()
        .filter(|(_, s)| is_mut(s)).map(|(i, _)| i).collect();
    for (a, &i) in mut_indices.iter().enumerate() {
        for &j in &mut_indices[a + 1..] {
            let exempt = allows(&specs[i], &specs[j].name) || allows(&specs[j], &specs[i].name);
            if exempt { continue; }
            let fa = &specs[i].name;
            let fb = &specs[j].name;
            checks.push(quote! {
                if accounts[#i].key == accounts[#j].key {
                    return Err(::verified_anchor::VAError::DuplicateAccount { field_a: #fa, field_b: #fb });
                }
            });
        }
    }

    // Emitted only when a `name.as_bytes()` seed actually needs it: an unused local `const`
    // would raise `dead_code` in every user crate that has no such seed.
    let instr_args_decl = if uses_arg_field(specs) {
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
    let index_of: std::collections::HashMap<String, usize> =
        specs.iter().enumerate().map(|(i, s)| (s.name.clone(), i)).collect();

    let mut lifecycle_steps: Vec<TokenStream2> = Vec::new();

    for (i, spec) in specs.iter().enumerate() {
        let fname = &spec.name;

        // Detect init: requires InitMarker + Payer + Space all present.
        let has_init = spec.constraints.iter().any(|c| matches!(c, Constraint::InitMarker));
        if has_init {
            let payer_ident = spec.constraints.iter().find_map(|c| {
                if let Constraint::Payer(p) = c { Some(p.to_string()) } else { None }
            });
            let space_val = spec.constraints.iter().find_map(|c| {
                if let Constraint::Space(n) = c { Some(*n) } else { None }
            });
            if let (Some(payer_name), Some(n)) = (payer_ident, space_val) {
                let pi = *index_of.get(&payer_name)
                    .unwrap_or_else(|| panic!("init payer `{payer_name}` is not a field of this struct"));
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
            if let Constraint::Close(dest) = c { Some(dest.to_string()) } else { None }
        });
        if let Some(dest_name) = close_dest {
            let di = *index_of.get(&dest_name)
                .unwrap_or_else(|| panic!("close destination `{dest_name}` is not a field of this struct"));
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
        let realloc_newlen = spec.constraints.iter().find_map(|c|
            if let Constraint::Realloc(n) = c { Some(*n) } else { None });
        if let Some(newlen) = realloc_newlen {
            let payer_name = spec.constraints.iter().find_map(|c|
                if let Constraint::ReallocPayer(p) = c { Some(p.to_string()) } else { None })
                .unwrap_or_else(|| panic!("realloc on `{}` needs realloc::payer", fname));
            let pi = *index_of.get(&payer_name)
                .unwrap_or_else(|| panic!("realloc::payer `{payer_name}` is not a field of this struct"));
            let zero_flag = spec.constraints.iter().any(|c| matches!(c, Constraint::ReallocZero(true)));
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
        let has_iin = spec.constraints.iter().any(|c| matches!(c, Constraint::InitIfNeeded));
        if has_iin {
            let payer_name = spec.constraints.iter().find_map(|c|
                if let Constraint::Payer(p) = c { Some(p.to_string()) } else { None })
                .unwrap_or_else(|| panic!("init_if_needed on `{}` needs payer", fname));
            let pi = *index_of.get(&payer_name)
                .unwrap_or_else(|| panic!("init_if_needed payer `{payer_name}` is not a field"));
            let n = spec.constraints.iter().find_map(|c|
                if let Constraint::Space(n) = c { Some(*n) } else { None })
                .unwrap_or_else(|| panic!("init_if_needed on `{}` needs space", fname));
            // The Task 6 struct-level guard requires an iin field to be a typed
            // `Account<'info, T>`; extract T so we can stamp its real Anchor discriminator
            // (making the fresh account a valid, type-correct, re-detectable account) and
            // reject a wrong-owner/undersized existing account in the ELSE branch.
            let t = match &spec.kind {
                WrapperKind::Account(t) => t.clone(),
                _ => panic!("init_if_needed on `{}` must be a typed Account (Task 6 guard)", fname),
            };
            // Seeded-PDA iin fields must be created with `invoke_signed` (the PDA is off-curve
            // and cannot sign as a tx signer). Re-derive the canonical bump inside
            // `execute_lifecycle` from the field's seed literals/field-keys and pass the signer
            // seeds. `arg(..)` seeds are NOT supported here — execute_lifecycle has no instr_data
            // — so reject them at compile time with a clear message.
            let seeds_for_signed: Option<Vec<TokenStream2>> = spec.constraints.iter()
                .find_map(|c| if let Constraint::Seeds(elems) = c { Some(elems) } else { None })
                .map(|elems| elems.iter().map(|se| match se {
                    SeedElem::Literal(b) => quote! { &#b[..] },
                    SeedElem::FieldKey(id) => {
                        // Guarded with a span in `derive_verified_accounts`, which runs first.
                        let fi = *index_of.get(&id.to_string())
                            .unwrap_or_else(|| unreachable!("seed field `{id}` reached codegen"));
                        quote! { accounts[#fi].key.as_ref() }
                    }
                    SeedElem::InstrArg(off, len) => unreachable!(
                        "init_if_needed + `arg({off}, {len})` seed on `{fname}` reached codegen"),
                    SeedElem::Unresolved(id) => unreachable!("unresolved seed `{id}` reached codegen"),
                    // Both instruction-data seed kinds are rejected with a span by the
                    // init_if_needed guard in `derive_verified_accounts`, which runs first.
                    SeedElem::ArgField(id, _) => unreachable!(
                        "init_if_needed + `{id}` arg seed on `{fname}` reached codegen"),
                }).collect());
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
                let Constraint::Seeds(elems) = c else { continue };
                for e in elems {
                    let SeedElem::Unresolved(id) = e else { continue };
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
                        return syn::Error::new_spanned(&*id, format!(
                            "seed `{id}.as_ref()` on field `{field}` names the #[instruction(...)] \
                             argument `{id}`, which verified-anchor had to drop: its type, or the \
                             type of an argument declared before it, is not one verified-anchor \
                             can locate yet, and every argument at or after the first such type \
                             has a Borsh offset that cannot be computed. Give those arguments \
                             mappable types, or move `{id}` ahead of them"))
                            .to_compile_error().into();
                    }
                    *e = match (is_arg, is_field) {
                        (true, true) => return syn::Error::new_spanned(&*id, format!(
                            "seed `{id}.as_ref()` on field `{field}` is ambiguous: `{id}` is both \
                             a declared #[instruction(...)] argument and an account field of this \
                             struct. Rename one of them — guessing would derive a different \
                             address than the one you meant"))
                            .to_compile_error().into(),
                        (true, false) => SeedElem::ArgField(id.clone(), ArgSeedForm::Bare),
                        (false, true) => SeedElem::FieldKey(id.clone()),
                        (false, false) => {
                            let args: Vec<&str> = instr_args.mappable.iter().map(|a| a.name.as_str()).collect();
                            return syn::Error::new_spanned(&*id, format!(
                                "seed `{id}.as_ref()` on field `{field}` names neither a declared \
                                 #[instruction(...)] argument (declared and mappable: [{}]) nor an \
                                 account field of this struct ([{}])",
                                args.join(", "), field_names.join(", ")))
                                .to_compile_error().into();
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
            let Constraint::Seeds(elems) = c else { continue };
            for e in elems {
                let SeedElem::ArgField(id, form) = e else { continue };
                let Some(arg) = instr_args.mappable.iter().find(|a| a.name == id.to_string()) else {
                    let mappable: Vec<&str> =
                        instr_args.mappable.iter().map(|a| a.name.as_str()).collect();
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
                    return syn::Error::new_spanned(id, format!(
                        "seed `{id}.{}` on field `{}` names an #[instruction(...)] argument that \
                         {why} (declared and mappable: [{}])",
                        form.spelling(), spec.name, mappable.join(", ")))
                        .to_compile_error().into();
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
                        ArgTyKind::Prefixed => "`as_bytes()` or `as_ref()` (String/Vec<u8> are length-prefixed)",
                        ArgTyKind::Numeric => "`to_le_bytes()` (Borsh integers are little-endian)",
                        ArgTyKind::Key => "`as_ref()` (a Pubkey is already byte-shaped)",
                        ArgTyKind::Other =>
                            "no seed spelling yet — seeds can be String/Vec<u8> (`as_bytes()` \
                             or `as_ref()`), an integer (`to_le_bytes()`) or a Pubkey \
                             (`as_ref()`). Note a `Vec<T>` with a wider element is NOT \
                             byte-shaped: its 4-byte prefix counts ELEMENTS, not bytes",
                    };
                    return syn::Error::new_spanned(id, format!(
                        "seed `{id}.{}` on field `{}` does not match the declared type of \
                         `{id}` in #[instruction(...)]; expected {expected}",
                        form.spelling(), spec.name))
                        .to_compile_error().into();
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
            let Constraint::Seeds(elems) = c else { continue };
            for e in elems {
                let SeedElem::FieldKey(id) = e else { continue };
                if !specs.iter().any(|s| s.name == id.to_string()) {
                    let fields: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
                    return syn::Error::new_spanned(id, format!(
                        "seed `{id}.key()` on field `{}` is not a field of this struct \
                         (fields: [{}])",
                        spec.name, fields.join(", ")))
                        .to_compile_error().into();
                }
            }
        }
    }

    // Guard: `init_if_needed` cannot combine with an instruction-data seed. `execute_lifecycle`
    // has no `instr_data` to derive the PDA from, so the signer seeds cannot be rebuilt there.
    // This is VALID Anchor source, so it must fail as a spanned compile error explaining the
    // limitation — not as the `panic!` inside `lifecycle_body`, which loses the span entirely.
    for spec in &specs {
        if !spec.constraints.iter().any(|c| matches!(c, Constraint::InitIfNeeded)) {
            continue;
        }
        for c in &spec.constraints {
            let Constraint::Seeds(elems) = c else { continue };
            let bad = elems.iter().find_map(|e| match e {
                SeedElem::ArgField(id, form) => Some(format!("{id}.{}", form.spelling())),
                SeedElem::InstrArg(off, len) => Some(format!("arg({off}, {len})")),
                _ => None,
            });
            if let Some(bad) = bad {
                return syn::Error::new_spanned(name, format!(
                    "field `{}` combines `init_if_needed` with the instruction-data seed `{bad}`, \
                     which verified-anchor cannot support: the account is created in \
                     `execute_lifecycle`, which receives no instruction data and so cannot \
                     rebuild the PDA's signer seeds. Use literal or `<field>.key()` seeds, or \
                     create the account with an explicit `init`",
                    spec.name))
                    .to_compile_error().into();
            }
        }
    }

    // Guard: `realloc` requires `mut` — realloc mutates the account data in place.
    // Guard: `init_if_needed` requires a typed `Account<'info, T>` — the wrapper's
    //        owner+discriminator checks are the reinit guard (prevents reuse attacks).
    for spec in &specs {
        let has_realloc = spec.constraints.iter().any(|c| matches!(c, Constraint::Realloc(_)));
        let has_mut = spec.constraints.iter().any(|c| matches!(c, Constraint::Mut));
        if has_realloc && !has_mut {
            return syn::Error::new_spanned(name,
                format!("field `{}` uses `realloc` but is not `mut`; realloc mutates the account", spec.name))
                .to_compile_error().into();
        }
        if spec.constraints.iter().any(|c| matches!(c, Constraint::InitIfNeeded))
            && !matches!(spec.kind, WrapperKind::Account(_)) {
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
        if spec.constraints.iter().any(|c| matches!(c, Constraint::HasOne(_)))
            && !matches!(spec.kind, WrapperKind::Account(_)) {
            return syn::Error::new_spanned(name,
                format!("field `{}` uses `has_one` but is not a typed `Account<'info, T>`; the target's Borsh offset comes from `T::LAYOUT`", spec.name))
                .to_compile_error().into();
        }
    }

    let body = validate_body(&specs, &instr_args.mappable);
    let (lean_tpl, lean_args) = lean_spec_string(&specs, &instr_args.mappable);
    let lifecycle = lifecycle_body(&specs);
    let has_lifecycle = specs.iter().any(|s| s.constraints.iter().any(|c|
        matches!(c, Constraint::InitMarker | Constraint::Close(_)
                  | Constraint::Realloc(_) | Constraint::InitIfNeeded)));
    let name_str = name.to_string();

    let has_info = !specs.is_empty();
    let bumps_struct_name = syn::Ident::new(&format!("{}Bumps", name), name.span());

    // Identify seeded fields (those with a Constraint::Seeds), preserving order.
    let seeded: Vec<(usize, &FieldSpec, &Vec<SeedElem>)> = specs.iter().enumerate()
        .filter_map(|(i, s)| s.constraints.iter().find_map(|c| {
            if let Constraint::Seeds(elems) = c { Some((i, s, elems)) } else { None }
        }))
        .collect();

    // Build name→index map for resolving `field.key()` seeds in Bumps init.
    let bumps_index_of: std::collections::HashMap<String, usize> =
        specs.iter().enumerate().map(|(i, s)| (s.name.clone(), i)).collect();

    // Per-seeded-field: (Bumps-field Ident, seed slice exprs, derivation program-id token).
    // The `seeds::program` override applies here too so the canonical bump exposed in `Bumps`
    // is derived against the SAME foreign program id used by `validate`.
    let bumps_fields: Vec<(syn::Ident, Vec<TokenStream2>, TokenStream2)> = seeded.iter().map(|(_, spec, elems)| {
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
    }).collect();

    let (bumps_struct_decl, bumps_struct_init) = if bumps_fields.is_empty() {
        (
            quote! { pub struct #bumps_struct_name; },
            quote! { #bumps_struct_name },
        )
    } else {
        let decl_fields: Vec<TokenStream2> = bumps_fields.iter().map(|(fname, _, _)| {
            quote! { pub #fname: u8 }
        }).collect();
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
    let accounts_impl_target = if has_info { quote! { #name<'info> } } else { quote! { #name } };
    let lean_spec_impl_target = if has_info { quote! { #name<'_> } } else { quote! { #name } };

    // The Bumps init inside `try_accounts` re-derives seeds, so it needs the same const.
    let try_accounts_instr_args = if uses_arg_field(&specs) {
        instr_args_const(&instr_args.mappable)
    } else {
        quote! {}
    };

    let expanded = quote! {
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
            }
        }
    };
    expanded.into()
}
