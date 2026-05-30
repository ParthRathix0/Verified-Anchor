use sha2::{Digest, Sha256};
use solana_program::account_info::AccountInfo;
use solana_program::pubkey::Pubkey;
use verified_anchor::{Validate, VAError, VerifiedAccounts};
use verified_anchor::{Signer, UncheckedAccount};

// Spec carrier: field names + #[account(..)] attrs define the constraints.
// Field types are driven by the wrapper kind; Signer<'info> implies signer check.
#[derive(VerifiedAccounts)]
struct Transfer<'info> {
    #[account(mut)]
    vault: UncheckedAccount<'info>,
    authority: Signer<'info>,
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
fn any_pid() -> Pubkey { Pubkey::new_unique() }

#[test]
fn accepts_valid() {
    let mut v = acct(false, true);
    let mut a = acct(true, false);
    let accts = [v.info(), a.info()];
    assert_eq!(Transfer::validate(&accts, &[], &any_pid()), Ok(()));
}
#[test]
fn rejects_non_writable_vault() {
    let mut v = acct(false, false);
    let mut a = acct(true, false);
    let accts = [v.info(), a.info()];
    assert_eq!(Transfer::validate(&accts, &[], &any_pid()), Err(VAError::NotWritable { field: "vault" }));
}
#[test]
fn rejects_non_signer_authority() {
    let mut v = acct(false, true);
    let mut a = acct(false, false);
    let accts = [v.info(), a.info()];
    assert_eq!(Transfer::validate(&accts, &[], &any_pid()), Err(VAError::MissingSigner { field: "authority" }));
}
#[test]
fn rejects_too_few_accounts() {
    let mut v = acct(false, true);
    let accts = [v.info()];
    assert_eq!(Transfer::validate(&accts, &[], &any_pid()), Err(VAError::NotEnoughAccounts { expected: 2, got: 1 }));
}
// Documents the permissiveness gap noted in docs/verified-anchor-bridge.md: the generated
// Rust accepts SURPLUS accounts (only the declared prefix is checked), whereas the Lean
// model/contract require an exact count. This is a transcription difference, not a soundness
// bug — the proof relates genValidate to the contract, both of which use exact equality.
#[test]
fn accepts_surplus_accounts() {
    let mut v = acct(false, true);   // vault: writable
    let mut a = acct(true, false);   // authority: signer
    let mut extra = acct(false, false);
    let accts = [v.info(), a.info(), extra.info()];   // 3 accounts, struct declares 2
    assert_eq!(Transfer::validate(&accts, &[], &any_pid()), Ok(()));
}

// Behavioral coverage for the owner constraint (the third M2 constraint kind). Distinct
// struct so it doesn't perturb Transfer's test vectors.
const PROG_OWNER: Pubkey = Pubkey::new_from_array([7u8; 32]);

#[derive(VerifiedAccounts)]
struct OwnedVault<'info> {
    #[account(owner = PROG_OWNER)]
    vault: UncheckedAccount<'info>,
}

fn acct_owned(owner: Pubkey) -> Acct {
    Acct { key: Pubkey::new_unique(), owner, lamports: 1, data: vec![], is_signer: false, is_writable: false }
}

#[test]
fn accepts_matching_owner() {
    let mut v = acct_owned(PROG_OWNER);
    let accts = [v.info()];
    assert_eq!(OwnedVault::validate(&accts, &[], &any_pid()), Ok(()));
}

#[test]
fn rejects_wrong_owner() {
    let mut v = acct_owned(Pubkey::new_from_array([9u8; 32]));   // not PROG_OWNER
    let accts = [v.info()];
    assert_eq!(OwnedVault::validate(&accts, &[], &any_pid()), Err(VAError::WrongOwner { field: "vault" }));
}

#[derive(VerifiedAccounts)]
struct CheckOwner<'info> {
    #[account(has_one = authority)]
    vault: UncheckedAccount<'info>,
    authority: UncheckedAccount<'info>,
}

fn acct_with_data(key: Pubkey, data: Vec<u8>) -> Acct {
    Acct { key, owner: Pubkey::new_unique(), lamports: 1, data, is_signer: false, is_writable: false }
}

#[test]
fn has_one_accepts_match() {
    let auth_key = Pubkey::new_unique();
    let mut data = vec![0u8; 8];                 // 8-byte discriminator
    data.extend_from_slice(auth_key.as_ref());   // authority Pubkey at offset 8
    let mut vault = acct_with_data(Pubkey::new_unique(), data);
    let mut authority = acct_with_data(auth_key, vec![]);
    let accts = [vault.info(), authority.info()];
    assert_eq!(CheckOwner::validate(&accts, &[], &any_pid()), Ok(()));
}

#[test]
fn has_one_rejects_mismatch() {
    let mut data = vec![0u8; 8];
    data.extend_from_slice(Pubkey::new_unique().as_ref());   // wrong stored authority
    let mut vault = acct_with_data(Pubkey::new_unique(), data);
    let mut authority = acct_with_data(Pubkey::new_unique(), vec![]);
    let accts = [vault.info(), authority.info()];
    assert_eq!(CheckOwner::validate(&accts, &[], &any_pid()), Err(VAError::WrongHasOne { field: "vault", target: "authority" }));
}

#[derive(VerifiedAccounts)]
struct PdaAccount<'info> {
    #[account(seeds = [b"vault", arg(0, 4)], bump)]
    pda: UncheckedAccount<'info>,
}

#[test]
fn seeds_accepts_canonical_pda() {
    let program_id = Pubkey::new_unique();
    let arg = [1u8, 2, 3, 4];
    let (pda, _bump) = Pubkey::find_program_address(&[b"vault", &arg], &program_id);
    let mut a = Acct { key: pda, owner: Pubkey::new_unique(), lamports: 1, data: vec![], is_signer: false, is_writable: false };
    let accts = [a.info()];
    assert_eq!(PdaAccount::validate(&accts, &arg, &program_id), Ok(()));
}

#[test]
fn seeds_rejects_wrong_pda() {
    let program_id = Pubkey::new_unique();
    let arg = [1u8, 2, 3, 4];
    let mut a = Acct { key: Pubkey::new_unique(), owner: Pubkey::new_unique(), lamports: 1, data: vec![], is_signer: false, is_writable: false };
    let accts = [a.info()];
    assert_eq!(PdaAccount::validate(&accts, &arg, &program_id), Err(VAError::WrongPda { field: "pda" }));
}

#[derive(VerifiedAccounts)]
struct PdaDeclaredBump<'info> {
    #[account(seeds = [b"vault"], bump = 0)]
    pda: UncheckedAccount<'info>,
}

#[test]
fn seeds_declared_bump_rejects_non_canonical() {
    let program_id = Pubkey::new_unique();
    let (pda, bump) = Pubkey::find_program_address(&[b"vault"], &program_id);
    // declared bump is 0; this fails unless the canonical bump happens to be 0.
    let mut a = Acct { key: pda, owner: Pubkey::new_unique(), lamports: 1, data: vec![], is_signer: false, is_writable: false };
    let accts = [a.info()];
    let res = PdaDeclaredBump::validate(&accts, &[], &program_id);
    if bump == 0 {
        assert_eq!(res, Ok(()));
    } else {
        assert_eq!(res, Err(VAError::WrongBump { field: "pda" }));
    }
}

fn disc(name: &str) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update(b"account:");
    h.update(name.as_bytes());
    let out = h.finalize();
    let mut d = [0u8; 8];
    d.copy_from_slice(&out[..8]);
    d
}

#[derive(VerifiedAccounts)]
struct DiscOnly<'info> {
    #[account(discriminator = "Vault")]
    vault: UncheckedAccount<'info>,
}

#[test]
fn discriminator_accepts_matching_prefix() {
    let mut v = Acct { key: Pubkey::new_unique(), owner: Pubkey::new_unique(), lamports: 1,
                       data: disc("Vault").to_vec(), is_signer: false, is_writable: false };
    let accts = [v.info()];
    assert_eq!(DiscOnly::validate(&accts, &[], &any_pid()), Ok(()));
}

#[test]
fn discriminator_rejects_wrong_prefix() {
    let mut v = Acct { key: Pubkey::new_unique(), owner: Pubkey::new_unique(), lamports: 1,
                       data: vec![0u8; 8], is_signer: false, is_writable: false };  // wrong disc (all zeros)
    let accts = [v.info()];
    assert_eq!(DiscOnly::validate(&accts, &[], &any_pid()),
               Err(VAError::WrongDiscriminator { field: "vault" }));
}

#[test]
fn discriminator_rejects_short_data() {
    let mut v = Acct { key: Pubkey::new_unique(), owner: Pubkey::new_unique(), lamports: 1,
                       data: vec![0u8; 4], is_signer: false, is_writable: false };  // too short
    let accts = [v.info()];
    assert_eq!(DiscOnly::validate(&accts, &[], &any_pid()),
               Err(VAError::WrongDiscriminator { field: "vault" }));
}

#[derive(borsh::BorshSerialize, borsh::BorshDeserialize, verified_anchor_macros::AccountData)]
struct Vault2 {
    pub authority: solana_program::pubkey::Pubkey,
    pub amount: u64,
}

#[test]
fn account_data_derive_computes_anchor_discriminator() {
    let expected = disc("Vault2"); // disc() from the M6 helper already in behavior.rs
    assert_eq!(<Vault2 as verified_anchor::AccountData>::DISCRIMINATOR, expected);
    let v = Vault2 { authority: solana_program::pubkey::Pubkey::new_from_array([7u8; 32]), amount: 42 };
    let bytes = borsh::to_vec(&v).unwrap();
    let v2: Vault2 = borsh::from_slice(&bytes).unwrap();
    assert_eq!(v2.amount, 42);
}
