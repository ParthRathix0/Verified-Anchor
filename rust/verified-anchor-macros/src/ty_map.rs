//! Rust-type -> `Ty` mapper, shared by `#[derive(AccountData)]` (M10 Task 5) and the
//! `#[instruction(...)]` argument-type mapping (M10 Task 9). Kept in its own file rather than
//! inlined in `account_data_derive.rs` so the second consumer does not need a copy-paste move.

use proc_macro2::TokenStream;
use quote::quote;
use syn::Type;

/// Map a Rust type to a runtime `Ty` expression and its Lean source form.
/// Unmappable types (floats, enums, user structs, anything unrecognised) return `None`; callers
/// that build a field list stop at the first `None` (see the derive's doc comment for why).
pub(crate) fn map_ty(ty: &Type) -> Option<(TokenStream, String)> {
    // `Type::Array` ([T; N]) is a distinct `syn::Type` variant from `Type::Path` and never
    // reaches the path-matching logic below, so it must be handled first. Fixed-size arrays
    // ([u8; 32] hashes/roots, [u8; 64] signatures, [Pubkey; N] collections) are common in real
    // Anchor account structs; without this arm a single such field returns `None` and the
    // derive's cutoff truncates the WHOLE descriptor there (see the derive's doc comment) —
    // every field after it silently falls out of the proven core.
    if let Type::Array(arr) = ty {
        let (rt, lean) = map_ty(&arr.elem)?;
        let n: usize = match &arr.len {
            syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(i), .. }) => i.base10_parse().ok()?,
            // A const-generic or named-const length (`[u8; N]`) cannot be evaluated at macro
            // expansion time — only integer literals can. Returning `None` here preserves the
            // safe cutoff instead of guessing a width, which would silently mis-offset every
            // field after it.
            _ => return None,
        };
        return Some((
            quote! { ::verified_anchor::layout::Ty::Array(&#rt, #n) },
            format!("(Ty.array {lean} {n})"),
        ));
    }

    let path = match ty {
        Type::Path(p) => p,
        _ => return None,
    };
    let seg = path.path.segments.last()?;
    let name = seg.ident.to_string();

    let simple = |rt: &str, lean: &str| {
        let id = syn::Ident::new(rt, proc_macro2::Span::call_site());
        Some((quote! { ::verified_anchor::layout::Ty::#id }, lean.to_string()))
    };

    match name.as_str() {
        "u8" => simple("U8", "Ty.u8"),
        "u16" => simple("U16", "Ty.u16"),
        "u32" => simple("U32", "Ty.u32"),
        "u64" => simple("U64", "Ty.u64"),
        "u128" => simple("U128", "Ty.u128"),
        "i8" => simple("I8", "Ty.i8"),
        "i16" => simple("I16", "Ty.i16"),
        "i32" => simple("I32", "Ty.i32"),
        "i64" => simple("I64", "Ty.i64"),
        "i128" => simple("I128", "Ty.i128"),
        "bool" => simple("Bool", "Ty.bool"),
        "Pubkey" => simple("Pubkey", "Ty.pubkey"),
        "String" => simple("String", "Ty.string"),
        "Vec" | "Option" => {
            let args = match &seg.arguments {
                syn::PathArguments::AngleBracketed(a) => a,
                _ => return None,
            };
            let inner = args.args.iter().find_map(|a| match a {
                syn::GenericArgument::Type(t) => Some(t),
                _ => None,
            })?;
            let (rt, lean) = map_ty(inner)?;
            if name == "Vec" {
                Some((quote! { ::verified_anchor::layout::Ty::Vec(&#rt) },
                      format!("(Ty.vec {lean})")))
            } else {
                Some((quote! { ::verified_anchor::layout::Ty::Option(&#rt) },
                      format!("(Ty.option {lean})")))
            }
        }
        _ => None,
    }
}
