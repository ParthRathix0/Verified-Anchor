use solana_program::account_info::AccountInfo;
use solana_program::pubkey::Pubkey;
use verified_anchor::{Validate, VAError, VerifiedAccounts};

// Spec carrier: field names + #[account(..)] attrs define the constraints.
// Field types are ignored by M2 codegen.
#[derive(VerifiedAccounts)]
struct Transfer {
    #[account(mut)]
    vault: u8,
    #[account(signer)]
    authority: u8,
}

struct Acct { key: Pubkey, owner: Pubkey, lamports: u64, data: Vec<u8>, is_signer: bool, is_writable: bool }
impl Acct {
    fn info(&mut self) -> AccountInfo {
        AccountInfo::new(&self.key, self.is_signer, self.is_writable,
            &mut self.lamports, &mut self.data, &self.owner, false, 0)
    }
}
fn acct(is_signer: bool, is_writable: bool) -> Acct {
    Acct { key: Pubkey::new_unique(), owner: Pubkey::new_unique(), lamports: 1, data: vec![], is_signer, is_writable }
}

#[test]
fn accepts_valid() {
    let mut v = acct(false, true);
    let mut a = acct(true, false);
    let accts = [v.info(), a.info()];
    assert_eq!(Transfer::validate(&accts), Ok(()));
}
#[test]
fn rejects_non_writable_vault() {
    let mut v = acct(false, false);
    let mut a = acct(true, false);
    let accts = [v.info(), a.info()];
    assert_eq!(Transfer::validate(&accts), Err(VAError::NotWritable { field: "vault" }));
}
#[test]
fn rejects_non_signer_authority() {
    let mut v = acct(false, true);
    let mut a = acct(false, false);
    let accts = [v.info(), a.info()];
    assert_eq!(Transfer::validate(&accts), Err(VAError::MissingSigner { field: "authority" }));
}
#[test]
fn rejects_too_few_accounts() {
    let mut v = acct(false, true);
    let accts = [v.info()];
    assert_eq!(Transfer::validate(&accts), Err(VAError::NotEnoughAccounts { expected: 2, got: 1 }));
}
