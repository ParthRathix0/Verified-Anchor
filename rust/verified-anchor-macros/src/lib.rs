use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{parse_macro_input, punctuated::Punctuated, Data, DeriveInput, Expr, Fields, Token};

/// One element of a `seeds = [...]` list.
enum SeedElem {
    Literal(syn::LitByteStr),   // b"vault"
    FieldKey(syn::Ident),       // field.key()
    InstrArg(usize, usize),     // arg(off, len)
}

/// One M2/M3 constraint parsed from a field's `#[account(...)]`.
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
            "owner" => {
                input.parse::<Token![=]>()?;
                let expr: Expr = input.parse()?;
                Ok(Constraint::Owner(expr))
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
                    let lit: syn::LitInt = input.parse()?;
                    Ok(Constraint::BumpDeclared(lit.base10_parse()?))
                } else {
                    Ok(Constraint::BumpCanonical)
                }
            }
            other => {
                let known_unsupported = [
                    "realloc", "zero", "rent_exempt", "constraint", "token", "mint",
                    "associated_token", "executable", "address", "owner_program",
                    "token_program", "seeds_program",
                ];
                let hint = if known_unsupported.contains(&other) {
                    format!("`{other}` is a stock-Anchor constraint that verified-anchor does not support")
                } else {
                    format!("unknown constraint `{other}`")
                };
                Err(syn::Error::new(
                    ident.span(),
                    format!("{hint}; verified-anchor supports: signer, mut, owner, has_one, init, payer, space, close, seeds, bump. See docs/migrating-from-anchor.md"),
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
        specs.push(FieldSpec { name, constraints });
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
        Constraint::BumpCanonical | Constraint::BumpDeclared(_) => String::new(),
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
        // If this field has a has_one constraint, emit a richer AccountType so the Lean
        // layout resolver can locate the stored Pubkey at offset 8 (after the discriminator).
        let ty = spec.constraints.iter().find_map(|c| {
            if let Constraint::HasOne(t) = c { Some(t.to_string()) } else { None }
        }).map(|t| format!("AccountType.account \"Vault\" [(\"{}\", 8)] Pubkey.zero", t))
          .unwrap_or_else(|| "AccountType.uncheckedAccount".to_string());
        let bump_str = spec.constraints.iter().find_map(|c| match c {
            Constraint::BumpCanonical => Some("BumpSpec.canonical".to_string()),
            Constraint::BumpDeclared(d) => Some(format!("BumpSpec.declared {}", d)),
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
        for c in &spec.constraints {
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
                // Lifecycle markers are handled in execute_lifecycle, not validate.
                Constraint::InitMarker | Constraint::Payer(_) | Constraint::Space(_) | Constraint::Close(_) => {
                    continue;
                }
                // Seeds/bump are handled in the per-field PDA block below.
                Constraint::Seeds(_) | Constraint::BumpCanonical | Constraint::BumpDeclared(_) => {
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
                    quote! { &instr_data[#off..#end] }
                }
            }).collect();
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
                    let (__pda, __bump) = ::solana_program::pubkey::Pubkey::find_program_address(__seeds, program_id);
                    if accounts[#i].key != &__pda {
                        return Err(::verified_anchor::VAError::WrongPda { field: #fname });
                    }
                    #bump_check
                }
            });
        }
    }
    quote! {
        fn validate(
            accounts: &[::solana_program::account_info::AccountInfo],
            instr_data: &[u8],
            program_id: &::solana_program::pubkey::Pubkey,
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
                        let ix = ::solana_program::system_instruction::create_account(
                            accounts[#pi].key, accounts[#i].key, rent_lamports, space_total as u64, program_id);
                        ::solana_program::program::invoke(&ix, accounts)
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
            accounts: &[::solana_program::account_info::AccountInfo],
            program_id: &::solana_program::pubkey::Pubkey,
            rent_lamports: u64,
        ) -> ::core::result::Result<(), ::verified_anchor::VAError> {
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
    let expanded = quote! {
        impl ::verified_anchor::Validate for #name {
            #body
        }
        impl #name {
            /// The Milestone-1 `AccountsStruct` literal for this struct (Lean source).
            pub fn lean_spec() -> ::std::string::String {
                #lean.to_string()
            }

            #lifecycle
        }
        ::verified_anchor::inventory::submit! {
            ::verified_anchor::SpecEntry {
                name: #name_str,
                lean_spec: #name::lean_spec,
                has_lifecycle: #has_lifecycle,
            }
        }
    };
    expanded.into()
}
