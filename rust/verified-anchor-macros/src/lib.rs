use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{parse_macro_input, punctuated::Punctuated, Data, DeriveInput, Expr, Fields, Token};

/// One M2 constraint parsed from a field's `#[account(...)]`.
enum Constraint {
    Signer,
    Mut,
    Owner(Expr),
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
            other => Err(syn::Error::new(
                ident.span(),
                format!("unsupported constraint `{other}` for Milestone 2 (only signer, mut, owner)"),
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
    }
}

fn lean_spec_string(specs: &[FieldSpec]) -> String {
    let mut fields = Vec::new();
    for spec in specs {
        let cs: Vec<String> = spec.constraints.iter().map(lean_constraint).collect();
        fields.push(format!(
            "{{ name := \"{}\", ty := AccountType.uncheckedAccount, constraints := [{}] }}",
            spec.name,
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
            };
            checks.push(check);
        }
    }
    quote! {
        fn validate(accounts: &[::solana_program::account_info::AccountInfo]) -> ::core::result::Result<(), ::verified_anchor::VAError> {
            if accounts.len() < #n {
                return Err(::verified_anchor::VAError::NotEnoughAccounts { expected: #n, got: accounts.len() });
            }
            #(#checks)*
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
    let expanded = quote! {
        impl ::verified_anchor::Validate for #name {
            #body
        }
        impl #name {
            /// The Milestone-1 `AccountsStruct` literal for this struct (Lean source).
            pub fn lean_spec() -> ::std::string::String {
                #lean.to_string()
            }
        }
    };
    expanded.into()
}
