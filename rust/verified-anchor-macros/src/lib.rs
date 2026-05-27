use proc_macro::TokenStream;

/// `#[derive(VerifiedAccounts)]` — generates `validate` and `lean_spec`.
/// Stub for now; real codegen lands in later tasks.
#[proc_macro_derive(VerifiedAccounts, attributes(account))]
pub fn derive_verified_accounts(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}
