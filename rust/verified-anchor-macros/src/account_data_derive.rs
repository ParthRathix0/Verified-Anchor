//! `#[derive(AccountData)]`: compute Anchor-wire DISCRIMINATOR. The user also
//! derives `BorshSerialize`/`BorshDeserialize` separately (Anchor's `#[account]`
//! attribute macro that bundles these is an M7b polish item).

use proc_macro::TokenStream;
use quote::quote;
use sha2::{Digest, Sha256};
use syn::{parse_macro_input, DeriveInput};

pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let mut h = Sha256::new();
    h.update(b"account:");
    h.update(name.to_string().as_bytes());
    let out = h.finalize();
    let bs: Vec<u8> = out[..8].to_vec();
    quote! {
        impl ::verified_anchor::AccountData for #name {
            const DISCRIMINATOR: [u8; 8] = [#(#bs),*];
        }
    }.into()
}
