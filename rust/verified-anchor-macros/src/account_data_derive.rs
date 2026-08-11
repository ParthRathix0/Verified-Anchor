//! `#[derive(AccountData)]`: the Anchor-wire DISCRIMINATOR plus the Borsh field layout.
//! The layout is what lets `has_one` and `constraint = <expr>` read the named field at its
//! real offset; before M10 the offset was hardcoded to 8.

use proc_macro::TokenStream;
use quote::quote;
use sha2::{Digest, Sha256};
use syn::{parse_macro_input, Data, DeriveInput, Fields};

use crate::ty_map::map_ty;

pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let mut h = Sha256::new();
    h.update(b"account:");
    h.update(name.to_string().as_bytes());
    let out = h.finalize();
    let bs: Vec<u8> = out[..8].to_vec();

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(n) => n.named.iter().collect::<Vec<_>>(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };

    let mut rt_entries = Vec::new();
    let mut lean_entries = Vec::new();
    for f in fields {
        let fname = match &f.ident {
            Some(i) => i.to_string(),
            None => continue,
        };
        // Stop at the first unmappable field: everything after it is unlocatable anyway,
        // because its offset depends on a width we cannot compute.
        let (rt, lean) = match map_ty(&f.ty) {
            Some(x) => x,
            None => break,
        };
        rt_entries.push(quote! { (#fname, #rt) });
        lean_entries.push(format!("(\"{fname}\", {lean})"));
    }

    let lean_lit = format!("(Ty.struct [{}])", lean_entries.join(", "));

    quote! {
        impl ::verified_anchor::AccountData for #name {
            const DISCRIMINATOR: [u8; 8] = [#(#bs),*];
            const LAYOUT: ::verified_anchor::layout::Ty =
                ::verified_anchor::layout::Ty::Struct(&[#(#rt_entries),*]);
            #[cfg(not(target_os = "solana"))]
            const LAYOUT_LEAN: &'static str = #lean_lit;
        }
    }
    .into()
}
