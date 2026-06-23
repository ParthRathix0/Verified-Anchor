use proc_macro::TokenStream;

mod account_data_derive;
mod account_attr;

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
    InstrArg(usize, usize),     // arg(off, len)
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
            other => {
                let known_unsupported = [
                    "realloc", "zero", "rent_exempt", "constraint", "token", "mint",
                    "associated_token", "owner_program",
                    "token_program", "seeds_program",
                ];
                let hint = if known_unsupported.contains(&other) {
                    format!("`{other}` is a stock-Anchor constraint that verified-anchor does not support")
                } else {
                    format!("unknown constraint `{other}`")
                };
                Err(syn::Error::new(
                    ident.span(),
                    format!("{hint}; verified-anchor supports: signer, mut, owner, has_one, init, payer, space, close, seeds, bump, discriminator, address, executable. See docs/migrating-from-anchor.md"),
                ))
            }
        }
    }
}

fn parse_seed_elem(e: Expr) -> syn::Result<SeedElem> {
    match e {
        Expr::Lit(syn::ExprLit { lit: syn::Lit::ByteStr(b), .. }) => Ok(SeedElem::Literal(b)),
        Expr::MethodCall(mc) if mc.method == "key" && mc.args.is_empty() => {
            if let Expr::Path(p) = mc.receiver.as_ref() {
                if let Some(id) = p.path.get_ident() {
                    return Ok(SeedElem::FieldKey(id.clone()));
                }
            }
            Err(syn::Error::new_spanned(mc.receiver, "seed `.key()` must be on a field name"))
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
            "unsupported seed (expected b\"..\", field.key(), or arg(off, len))")),
    }
}

fn lit_usize(e: Option<&Expr>) -> syn::Result<usize> {
    match e {
        Some(Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(i), .. })) => i.base10_parse(),
        _ => Err(syn::Error::new(proc_macro2::Span::call_site(),
            "arg(off, len) needs two integer literals")),
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
            }).collect();
            format!("Constraint.seeds [{}] @@BUMP@@", seeds.join(", "))
        }
        Constraint::BumpCanonical | Constraint::BumpDeclared(_) | Constraint::BumpStored(_) => String::new(),
        Constraint::Discriminator(d) => {
            let bytes: Vec<String> = d.iter().map(|x| x.to_string()).collect();
            format!("Constraint.discriminator (ByteArray.mk #[{}])", bytes.join(", "))
        }
        // Schematic placeholder: the theorem is ∀ over the pubkey (same trick as `owner`).
        Constraint::Address(_) => "Constraint.address Pubkey.zero".to_string(),
        Constraint::Executable => "Constraint.executable".to_string(),
        Constraint::ProgramMarker(_) => String::new(),
    }
}

fn lean_spec_string(specs: &[FieldSpec]) -> String {
    let mut fields = Vec::new();
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
        let ty = match &spec.kind {
            WrapperKind::Account(t) => {
                let layout = spec.constraints.iter().find_map(|c| {
                    if let Constraint::HasOne(target) = c { Some(target.to_string()) } else { None }
                });
                let lay = match layout {
                    Some(target) => format!("[(\"{}\", 8)]", target),
                    None => "[]".to_string(),
                };
                format!("AccountType.account \"{}\" {} Pubkey.zero", t, lay)
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
        let cs_joined = cs.join(", ").replace("@@BUMP@@", &bump_str);
        fields.push(format!(
            "{{ name := \"{}\", ty := {}, constraints := [{}] }}",
            spec.name,
            ty,
            cs_joined
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
    format!("{{ programId := Pubkey.zero, fields :={} }}", body)
}

fn validate_body(specs: &[FieldSpec]) -> TokenStream2 {
    let n = specs.len();
    // Build a name→index map so has_one can look up the target field's position.
    let index_of: std::collections::HashMap<String, usize> =
        specs.iter().enumerate().map(|(i, s)| (s.name.clone(), i)).collect();
    let mut checks = Vec::new();
    for (i, spec) in specs.iter().enumerate() {
        let name = &spec.name;
        let implied = wrapper_implied(&spec.kind);
        let effective: Vec<Constraint> = implied.into_iter()
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
                    quote! {
                        {
                            let data = accounts[#i].try_borrow_data()
                                .map_err(|_| ::verified_anchor::VAError::WrongHasOne { field: #fname, target: #tname })?;
                            if data.len() < 8 + 32 || &data[8..8 + 32] != accounts[#tidx].key.as_ref() {
                                return Err(::verified_anchor::VAError::WrongHasOne { field: #fname, target: #tname });
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
                // Lifecycle markers are handled in execute_lifecycle, not validate.
                Constraint::InitMarker | Constraint::Payer(_) | Constraint::Space(_) | Constraint::Close(_) => {
                    continue;
                }
                // Seeds/bump are handled in the per-field PDA block below.
                Constraint::Seeds(_) | Constraint::BumpCanonical | Constraint::BumpDeclared(_)
                | Constraint::BumpStored(_) => {
                    continue;
                }
            };
            checks.push(check);
        }

        // seeds/bump: emit one PDA check per field that declares `seeds`.
        if let Some(Constraint::Seeds(elems)) = spec.constraints.iter()
            .find(|c| matches!(c, Constraint::Seeds(_)))
        {
            let fname = name;
            let seed_exprs: Vec<TokenStream2> = elems.iter().map(|se| match se {
                SeedElem::Literal(b) => quote! { &#b[..] },
                SeedElem::FieldKey(id) => {
                    let fi = *index_of.get(&id.to_string())
                        .unwrap_or_else(|| panic!("seed field `{}` is not a field of this struct", id));
                    quote! { accounts[#fi].key.as_ref() }
                }
                SeedElem::InstrArg(off, len) => {
                    let end = off + len;
                    // Clamp to length so a short `instr_data` cannot panic; this mirrors the
                    // Lean model's `ByteArray.extract off (off+len)` (which clamps both bounds).
                    quote! { &instr_data[(#off).min(instr_data.len())..(#end).min(instr_data.len())] }
                }
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
                            &[ #(#seed_exprs,)* &[__stored_bump] ], program_id)
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
                        let (__pda, __bump) = ::verified_anchor::solana_program::pubkey::Pubkey::find_program_address(__seeds, program_id);
                        if accounts[#i].key != &__pda {
                            return Err(::verified_anchor::VAError::WrongPda { field: #fname });
                        }
                        #bump_check
                    }
                });
            }
        }
    }
    quote! {
        fn validate(
            accounts: &[::verified_anchor::solana_program::account_info::AccountInfo],
            instr_data: &[u8],
            program_id: &::verified_anchor::solana_program::pubkey::Pubkey,
        ) -> ::core::result::Result<(), ::verified_anchor::VAError> {
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

#[proc_macro_derive(VerifiedAccounts, attributes(account))]
pub fn derive_verified_accounts(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let specs = match collect_fields(&input) {
        Ok(s) => s,
        Err(e) => return e.to_compile_error().into(),
    };
    let name = &input.ident;
    let body = validate_body(&specs);
    let lean = lean_spec_string(&specs);
    let lifecycle = lifecycle_body(&specs);
    let has_lifecycle = specs.iter().any(|s| s.constraints.iter().any(|c|
        matches!(c, Constraint::InitMarker | Constraint::Close(_))));
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

    // Per-seeded-field: (Bumps-field Ident, TokenStream Vec for the seed slice exprs).
    let bumps_fields: Vec<(syn::Ident, Vec<TokenStream2>)> = seeded.iter().map(|(_, spec, elems)| {
        let fname = syn::Ident::new(&spec.name, name.span());
        let seed_exprs: Vec<TokenStream2> = elems.iter().map(|se| match se {
            SeedElem::Literal(b) => quote! { &#b[..] },
            SeedElem::FieldKey(id) => {
                let fi = *bumps_index_of.get(&id.to_string())
                    .unwrap_or_else(|| panic!("seed field `{}` is not a field of this struct", id));
                quote! { accounts[#fi].key.as_ref() }
            }
            SeedElem::InstrArg(off, len) => {
                let end = off + len;
                // Clamp to length (matches the validate-side seed slice and the Lean model).
                quote! { &instr_data[(#off).min(instr_data.len())..(#end).min(instr_data.len())] }
            }
        }).collect();
        (fname, seed_exprs)
    }).collect();

    let (bumps_struct_decl, bumps_struct_init) = if bumps_fields.is_empty() {
        (
            quote! { pub struct #bumps_struct_name; },
            quote! { #bumps_struct_name },
        )
    } else {
        let decl_fields: Vec<TokenStream2> = bumps_fields.iter().map(|(fname, _)| {
            quote! { pub #fname: u8 }
        }).collect();
        let init_fields: Vec<TokenStream2> = bumps_fields.iter().map(|(fname, seed_exprs)| {
            quote! {
                #fname: {
                    let __seeds: &[&[u8]] = &[ #(#seed_exprs),* ];
                    let (_pda, __b) = ::verified_anchor::solana_program::pubkey::Pubkey::find_program_address(__seeds, program_id);
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

    let expanded = quote! {
        #validate_impl
        impl #lean_spec_impl_target {
            /// The Milestone-1 `AccountsStruct` literal for this struct (Lean source).
            pub fn lean_spec() -> ::std::string::String {
                #lean.to_string()
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
