use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{parse_macro_input, punctuated::Punctuated, Data, DeriveInput, Expr, Fields, Token};

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
            other => Err(syn::Error::new(
                ident.span(),
                format!("unsupported constraint `{other}` (supported: signer, mut, owner, has_one, init, payer, space, close)"),
            )),
        }
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
    }
}

fn lean_spec_string(specs: &[FieldSpec]) -> String {
    let mut fields = Vec::new();
    for spec in specs {
        let cs: Vec<String> = spec.constraints.iter()
            .map(lean_constraint)
            .filter(|s| !s.is_empty())
            .collect();
        // If this field has a has_one constraint, emit a richer AccountType so the Lean
        // layout resolver can locate the stored Pubkey at offset 8 (after the discriminator).
        let ty = spec.constraints.iter().find_map(|c| {
            if let Constraint::HasOne(t) = c { Some(t.to_string()) } else { None }
        }).map(|t| format!("AccountType.account \"Vault\" [(\"{}\", 8)] Pubkey.zero", t))
          .unwrap_or_else(|| "AccountType.uncheckedAccount".to_string());
        fields.push(format!(
            "{{ name := \"{}\", ty := {}, constraints := [{}] }}",
            spec.name,
            ty,
            cs.join(", ")
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
    format!("{{ programId := Pubkey.zero\n, fields :={} }}", body)
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
            };
            checks.push(check);
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
    };
    expanded.into()
}
