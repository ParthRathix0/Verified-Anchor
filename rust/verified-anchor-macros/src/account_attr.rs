//! `#[account]` attribute macro — bundles `BorshSerialize + BorshDeserialize + AccountData`
//! so users write `#[account]` instead of three derives. Mirrors stock Anchor's sugar.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Item};

pub fn account(args: TokenStream, input: TokenStream) -> TokenStream {
    let args_tokens: proc_macro2::TokenStream = args.into();
    if !args_tokens.is_empty() {
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            "verified-anchor: `#[account]` takes no arguments in M7b; the bundled derives are fixed (BorshSerialize, BorshDeserialize, AccountData). Use the explicit 3-derive form if you need different derive flags."
        ).to_compile_error().into();
    }
    let item = parse_macro_input!(input as Item);
    let item_struct = match item {
        Item::Struct(s) => s,
        _ => return syn::Error::new(
            proc_macro2::Span::call_site(),
            "verified-anchor: `#[account]` may only be applied to a named-fields struct"
        ).to_compile_error().into(),
    };
    let expanded = quote! {
        #[derive(::borsh::BorshSerialize, ::borsh::BorshDeserialize, ::verified_anchor::AccountData)]
        #item_struct
    };
    expanded.into()
}
