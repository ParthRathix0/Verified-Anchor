use sha2::{Digest, Sha256};
use solana_program::account_info::AccountInfo;
use solana_program::pubkey::Pubkey;
use verified_anchor::{Validate, VAError, VerifiedAccounts};
use verified_anchor::{Signer, UncheckedAccount};

// Provide a crate::ID so that Account<'info, T> (which implies owner=crate::ID)
// resolves in this test binary. Must be a valid base58 pubkey string of length 44.
solana_program::declare_id!("VATest1111111111111111111111111111111111111");

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

/// `has_one` needs a typed `Account<'info, T>`: the target's Borsh offset comes from
/// `T::LAYOUT`, and an untyped wrapper has no layout to walk (M10 Task 7). This fixture used
/// to be an `UncheckedAccount` relying on the hardcoded offset-8 read.
#[verified_anchor::account]
struct OwnerVault {
    authority: Pubkey,
}

#[derive(VerifiedAccounts)]
struct CheckOwner<'info> {
    #[account(has_one = authority)]
    vault: verified_anchor::Account<'info, OwnerVault>,
    authority: UncheckedAccount<'info>,
}

fn acct_with_data(key: Pubkey, data: Vec<u8>) -> Acct {
    Acct { key, owner: Pubkey::new_unique(), lamports: 1, data, is_signer: false, is_writable: false }
}

/// Real Anchor wire bytes for an `OwnerVault`: 8-byte discriminator then the authority key.
fn owner_vault_data(authority: Pubkey) -> Vec<u8> {
    let mut d = <OwnerVault as verified_anchor::AccountData>::DISCRIMINATOR.to_vec();
    d.extend_from_slice(authority.as_ref());
    d
}

#[test]
fn has_one_accepts_match() {
    let auth_key = Pubkey::new_unique();
    let mut vault = acct_with_data(Pubkey::new_unique(), owner_vault_data(auth_key));
    // `Account<'info, T>` implies `owner = crate::ID`; without it validate stops before has_one.
    vault.owner = crate::ID;
    let mut authority = acct_with_data(auth_key, vec![]);
    let accts = [vault.info(), authority.info()];
    assert_eq!(CheckOwner::validate(&accts, &[], &any_pid()), Ok(()));
}

#[test]
fn has_one_rejects_mismatch() {
    // wrong stored authority
    let mut vault = acct_with_data(Pubkey::new_unique(), owner_vault_data(Pubkey::new_unique()));
    vault.owner = crate::ID;
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

#[derive(VerifiedAccounts)]
struct PdaStoredBump<'info> {
    #[account(seeds = [b"vault"], bump = arg(0))]
    pda: UncheckedAccount<'info>,
}

/// Find a GENUINELY NON-CANONICAL off-curve bump: one strictly below the canonical bump.
/// `find_program_address` returns the HIGHEST off-curve bump, so we search ascending from 0
/// up to (but not including) `canon_bump` for the first b where `create_program_address`
/// succeeds.  The resulting address is DIFFERENT from the canonical PDA (proven by
/// `assert_ne!` in the caller).
fn non_canonical_stored_bump(program_id: &Pubkey) -> (u8, Pubkey) {
    let (_canon_key, canon_bump) = Pubkey::find_program_address(&[b"vault"], program_id);
    for b in 0u8..canon_bump {
        if let Ok(pk) = Pubkey::create_program_address(&[b"vault", &[b]], program_id) {
            return (b, pk);
        }
    }
    panic!("no non-canonical off-curve bump found below canonical bump {canon_bump} for b\"vault\" — change the seed literal");
}

#[test]
fn seeds_stored_bump_accepts_matching_pda() {
    let program_id = Pubkey::new_unique();
    let (canon_key, canon_bump) = Pubkey::find_program_address(&[b"vault"], &program_id);
    let (bump, pda) = non_canonical_stored_bump(&program_id);
    // The stored bump MUST be strictly below the canonical bump, and the derived address
    // MUST differ from the canonical PDA — this proves we are exercising a genuinely
    // non-canonical PDA, not just re-testing what a canonical validator would also accept.
    assert!(bump < canon_bump, "stored bump {bump} must be below canonical bump {canon_bump}");
    assert_ne!(pda, canon_key, "non-canonical PDA must differ from canonical PDA");
    // instr data byte 0 is the stored bump.
    let mut a = Acct { key: pda, owner: Pubkey::new_unique(), lamports: 1, data: vec![], is_signer: false, is_writable: false };
    let accts = [a.info()];
    assert_eq!(PdaStoredBump::validate(&accts, &[bump], &program_id), Ok(()));
}

#[test]
fn seeds_stored_bump_rejects_wrong_pda() {
    let program_id = Pubkey::new_unique();
    let (bump, _pda) = non_canonical_stored_bump(&program_id);
    let mut a = Acct { key: Pubkey::new_unique(), owner: Pubkey::new_unique(), lamports: 1, data: vec![], is_signer: false, is_writable: false };
    let accts = [a.info()];
    assert_eq!(PdaStoredBump::validate(&accts, &[bump], &program_id), Err(VAError::WrongPda { field: "pda" }));
}

#[test]
fn seeds_stored_bump_rejects_short_instr_data() {
    let program_id = Pubkey::new_unique();
    let (_bump, pda) = non_canonical_stored_bump(&program_id);
    // empty instr data => no byte at offset 0 => clean reject (mirrors the Lean none-safe spec).
    let mut a = Acct { key: pda, owner: Pubkey::new_unique(), lamports: 1, data: vec![], is_signer: false, is_writable: false };
    let accts = [a.info()];
    assert_eq!(PdaStoredBump::validate(&accts, &[], &program_id), Err(VAError::WrongPda { field: "pda" }));
}

const FOREIGN_PROGRAM: Pubkey = Pubkey::new_from_array([9u8; 32]);

/// `seeds::program = <expr>` derives the PDA against the FOREIGN program id, not the struct's
/// own `program_id`. The Lean model carries this as the third `Constraint.seeds` field.
#[derive(VerifiedAccounts)]
struct PdaForeignProgram<'info> {
    #[account(seeds = [b"vault"], seeds::program = FOREIGN_PROGRAM, bump)]
    pda: UncheckedAccount<'info>,
}

#[test]
fn seeds_program_accepts_foreign_derived_pda() {
    // The struct is invoked under THIS program id, but the PDA is derived against FOREIGN_PROGRAM.
    let program_id = Pubkey::new_unique();
    let (foreign_pda, _b) = Pubkey::find_program_address(&[b"vault"], &FOREIGN_PROGRAM);
    let mut a = Acct { key: foreign_pda, owner: Pubkey::new_unique(), lamports: 1, data: vec![], is_signer: false, is_writable: false };
    let accts = [a.info()];
    assert_eq!(PdaForeignProgram::validate(&accts, &[], &program_id), Ok(()));
}

#[test]
fn seeds_program_rejects_own_program_pda() {
    // The PDA derived against the struct's OWN program id is the WRONG one here — the override
    // must derive against FOREIGN_PROGRAM, so this is rejected (proves the override actually bites).
    let program_id = Pubkey::new_unique();
    let (own_pda, _b) = Pubkey::find_program_address(&[b"vault"], &program_id);
    let (foreign_pda, _fb) = Pubkey::find_program_address(&[b"vault"], &FOREIGN_PROGRAM);
    assert_ne!(own_pda, foreign_pda, "own-program PDA must differ from the foreign-program PDA");
    let mut a = Acct { key: own_pda, owner: Pubkey::new_unique(), lamports: 1, data: vec![], is_signer: false, is_writable: false };
    let accts = [a.info()];
    assert_eq!(PdaForeignProgram::validate(&accts, &[], &program_id), Err(VAError::WrongPda { field: "pda" }));
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

// ── Task 1: try_accounts Borsh round-trip ────────────────────────────────────────────
//
// VaultDataStruct uses Account<'info, Vault2> which auto-implies:
//   owner = crate::ID  (satisfied by the declare_id! near the top of this file)
//   discriminator = sha256("account:Vault2")[..8]
// try_accounts calls validate first (owner + discriminator checks), then Borsh-deserialises.

#[derive(VerifiedAccounts)]
struct VaultDataStruct<'info> {
    vault: verified_anchor::Account<'info, Vault2>,
}

#[test]
fn try_accounts_deserializes_typed_data() {
    use verified_anchor::Accounts;
    let v = Vault2 {
        authority: Pubkey::new_from_array([7u8; 32]),
        amount: 999,
    };
    let mut data = disc("Vault2").to_vec();
    data.extend(borsh::to_vec(&v).unwrap());
    let mut a = Acct {
        key: Pubkey::new_unique(),
        owner: crate::ID,   // satisfies Account<T>'s implied owner=crate::ID
        lamports: 1,
        data,
        is_signer: false,
        is_writable: false,
    };
    let accts = [a.info()];
    let result = <VaultDataStruct as Accounts>::try_accounts(&crate::ID, &accts, &[]);
    let (parsed, _bumps) = result.expect("try_accounts should succeed with valid disc + payload");
    assert_eq!(parsed.vault.data.amount, 999);
    assert_eq!(parsed.vault.data.authority, Pubkey::new_from_array([7u8; 32]));
}

#[test]
fn try_accounts_borsh_failed_on_truncated_data() {
    use verified_anchor::Accounts;
    // Only 8 discriminator bytes, no Borsh payload → BorshFailed
    let data = disc("Vault2").to_vec();
    let mut a = Acct {
        key: Pubkey::new_unique(),
        owner: crate::ID,
        lamports: 1,
        data,
        is_signer: false,
        is_writable: false,
    };
    let accts = [a.info()];
    let result = <VaultDataStruct as Accounts>::try_accounts(&crate::ID, &accts, &[]);
    assert_eq!(result.err(), Some(VAError::BorshFailed { field: "vault" }));
}

// ── Task 2: SystemAccount + Program<P> wrapper-reject tests ─────────────────────────

#[derive(VerifiedAccounts)]
struct SysAccountField<'info> {
    sys: verified_anchor::SystemAccount<'info>,
}

#[test]
fn system_account_accepts_system_owner() {
    let mut a = Acct {
        key: Pubkey::new_unique(),
        owner: solana_program::system_program::ID,
        lamports: 1,
        data: vec![],
        is_signer: false,
        is_writable: false,
    };
    let accts = [a.info()];
    assert_eq!(SysAccountField::validate(&accts, &[], &any_pid()), Ok(()));
}

#[test]
fn system_account_rejects_non_system_owner() {
    let mut a = Acct {
        key: Pubkey::new_unique(),
        owner: Pubkey::new_unique(),   // not system program
        lamports: 1,
        data: vec![],
        is_signer: false,
        is_writable: false,
    };
    let accts = [a.info()];
    assert_eq!(
        SysAccountField::validate(&accts, &[], &any_pid()),
        Err(VAError::WrongOwner { field: "sys" })
    );
}

#[derive(VerifiedAccounts)]
struct ProgField<'info> {
    sys: verified_anchor::Program<'info, verified_anchor::System>,
}

#[test]
fn program_accepts_executable_with_correct_key() {
    let key = solana_program::system_program::ID;   // matches System::ID
    let owner = Pubkey::new_unique();
    let mut lamports = 1u64;
    let mut data: Vec<u8> = vec![];
    let info = AccountInfo::new(&key, false, false, &mut lamports, &mut data, &owner, true, 0);
    let accts = [info];
    assert_eq!(ProgField::validate(&accts, &[], &any_pid()), Ok(()));
}

#[test]
fn program_rejects_non_executable() {
    let key = solana_program::system_program::ID;
    let owner = Pubkey::new_unique();
    let mut lamports = 1u64;
    let mut data: Vec<u8> = vec![];
    // executable = false
    let info = AccountInfo::new(&key, false, false, &mut lamports, &mut data, &owner, false, 0);
    let accts = [info];
    assert_eq!(
        ProgField::validate(&accts, &[], &any_pid()),
        Err(VAError::WrongOwner { field: "sys" })
    );
}

#[test]
fn program_rejects_wrong_key() {
    let wrong_key = Pubkey::new_unique();   // not system_program::ID
    let owner = Pubkey::new_unique();
    let mut lamports = 1u64;
    let mut data: Vec<u8> = vec![];
    let info = AccountInfo::new(&wrong_key, false, false, &mut lamports, &mut data, &owner, true, 0);
    let accts = [info];
    assert_eq!(
        ProgField::validate(&accts, &[], &any_pid()),
        Err(VAError::WrongOwner { field: "sys" })
    );
}

#[derive(VerifiedAccounts)]
struct WithPda<'info> {
    #[account(seeds = [b"vault", arg(0, 4)], bump)]
    pda: verified_anchor::UncheckedAccount<'info>,
}

#[test]
fn bumps_struct_carries_canonical_bump() {
    use verified_anchor::Accounts;
    let program_id = Pubkey::new_unique();
    let arg = [1u8, 2, 3, 4];
    let (pda, expected_bump) = Pubkey::find_program_address(&[b"vault", &arg], &program_id);
    let mut a = Acct { key: pda, owner: Pubkey::new_unique(), lamports: 1, data: vec![], is_signer: false, is_writable: false };
    let accts = [a.info()];
    let (_struct, bumps) = <WithPda as Accounts>::try_accounts(&program_id, &accts, &arg).unwrap();
    assert_eq!(bumps.pda, expected_bump);
}

#[test]
fn seeds_short_instr_data_does_not_panic() {
    // `arg(0, 4)` on EMPTY instruction data must not panic. The generated slice is clamped to
    // the data length (mirroring the Lean `ByteArray.extract`), so validation cleanly rejects
    // with WrongPda instead of an out-of-bounds slice panic.
    let mut a = Acct {
        key: Pubkey::new_unique(),
        owner: Pubkey::new_unique(),
        lamports: 1,
        data: vec![],
        is_signer: false,
        is_writable: false,
    };
    let accts = [a.info()];
    assert_eq!(
        WithPda::validate(&accts, &[], &any_pid()),
        Err(VAError::WrongPda { field: "pda" })
    );
}

#[derive(VerifiedAccounts)]
struct LifecycleGuard<'info> {
    #[account(close = dest)]
    target: UncheckedAccount<'info>,
    #[account(mut)]
    dest: UncheckedAccount<'info>,
}

#[test]
fn execute_lifecycle_rejects_short_accounts() {
    // execute_lifecycle indexes accounts by field position. On too few accounts it must
    // return NotEnoughAccounts (mirroring the Lean none-safety), not panic on an OOB index.
    assert_eq!(
        LifecycleGuard::execute_lifecycle(&[], &any_pid(), 0),
        Err(VAError::NotEnoughAccounts { expected: 2, got: 0 })
    );
}

// ── Task 1 (M8.1): explicit address / executable annotations ─────────────────────────

const EXPECTED_ID: Pubkey = Pubkey::new_from_array([0xABu8; 32]);

#[derive(VerifiedAccounts)]
struct WithAddr<'info> {
    #[account(address = crate::EXPECTED_ID)]
    cfg: UncheckedAccount<'info>,
    #[account(executable)]
    prog: UncheckedAccount<'info>,
}

// Note: AccountInfo::new last-but-one bool is `executable`.
fn make_info_exec(a: &mut Acct, executable: bool) -> AccountInfo {
    AccountInfo::new(&a.key, a.is_signer, a.is_writable,
        &mut a.lamports, &mut a.data, &a.owner, executable, 0)
}

#[test]
fn address_and_executable_accept_valid() {
    let mut cfg = Acct { key: EXPECTED_ID, owner: Pubkey::new_unique(), lamports: 1,
                         data: vec![], is_signer: false, is_writable: false };
    let mut prog = Acct { key: Pubkey::new_unique(), owner: Pubkey::new_unique(), lamports: 1,
                          data: vec![], is_signer: false, is_writable: false };
    let cfg_info = make_info_exec(&mut cfg, false);
    let prog_info = make_info_exec(&mut prog, true);
    let accts = [cfg_info, prog_info];
    assert_eq!(WithAddr::validate(&accts, &[], &any_pid()), Ok(()));
}

#[test]
fn address_rejects_wrong_key() {
    let wrong_key = Pubkey::new_unique();  // not EXPECTED_ID
    let mut cfg = Acct { key: wrong_key, owner: Pubkey::new_unique(), lamports: 1,
                         data: vec![], is_signer: false, is_writable: false };
    let mut prog = Acct { key: Pubkey::new_unique(), owner: Pubkey::new_unique(), lamports: 1,
                          data: vec![], is_signer: false, is_writable: false };
    let cfg_info = make_info_exec(&mut cfg, false);
    let prog_info = make_info_exec(&mut prog, true);
    let accts = [cfg_info, prog_info];
    assert_eq!(WithAddr::validate(&accts, &[], &any_pid()),
               Err(VAError::WrongAddress { field: "cfg" }));
}

#[test]
fn executable_rejects_non_executable() {
    let mut cfg = Acct { key: EXPECTED_ID, owner: Pubkey::new_unique(), lamports: 1,
                         data: vec![], is_signer: false, is_writable: false };
    let mut prog = Acct { key: Pubkey::new_unique(), owner: Pubkey::new_unique(), lamports: 1,
                          data: vec![], is_signer: false, is_writable: false };
    let cfg_info = make_info_exec(&mut cfg, false);
    let prog_info = make_info_exec(&mut prog, false);  // not executable
    let accts = [cfg_info, prog_info];
    assert_eq!(WithAddr::validate(&accts, &[], &any_pid()),
               Err(VAError::NotExecutable { field: "prog" }));
}

#[verified_anchor::account]
pub struct VaultAttr { pub authority: solana_program::pubkey::Pubkey, pub amount: u64 }

#[test]
fn account_attribute_implies_borsh_and_discriminator() {
    let d = <VaultAttr as verified_anchor::AccountData>::DISCRIMINATOR;
    assert_eq!(d, disc("VaultAttr"));
    let v = VaultAttr { authority: solana_program::pubkey::Pubkey::new_from_array([7u8; 32]), amount: 42 };
    let bytes = borsh::to_vec(&v).unwrap();
    let v2: VaultAttr = borsh::from_slice(&bytes).unwrap();
    assert_eq!(v2.amount, 42);
    assert_eq!(v2.authority, v.authority);
}

// ---- M8.5: rent_exempt = enforce / skip ----
//
// Native tests can't call Rent::get() (no sysvar runtime). So we only verify:
//   1. Structs with `rent_exempt = enforce` and `rent_exempt = skip` derive correctly.
//   2. lean_spec() output for each is as expected (Constraint.rentExempt vs no rent entry).
// The empirical on-chain reject/accept lives in runtime_rent.rs (litesvm).

#[derive(VerifiedAccounts)]
struct RentEnforce<'info> {
    #[account(rent_exempt = enforce)]
    vault: UncheckedAccount<'info>,
}

#[derive(VerifiedAccounts)]
struct RentSkip<'info> {
    #[account(rent_exempt = skip)]
    vault: UncheckedAccount<'info>,
}

#[test]
fn rent_enforce_lean_spec_contains_rentexempt() {
    let spec = RentEnforce::lean_spec();
    assert!(
        spec.contains("Constraint.rentExempt"),
        "rent_exempt = enforce must emit Constraint.rentExempt in lean_spec; got: {spec}"
    );
}

#[test]
fn rent_skip_lean_spec_has_no_rentexempt() {
    let spec = RentSkip::lean_spec();
    assert!(
        !spec.contains("rentExempt"),
        "rent_exempt = skip must NOT emit any rentExempt in lean_spec; got: {spec}"
    );
}

// ---- M8.4: struct-level distinct mutable keys + explicit opt-out ----

// Two writable accounts. The macro auto-adds the pairwise distinct-key check.
#[derive(VerifiedAccounts)]
struct DupMut<'info> {
    #[account(mut)]
    a: UncheckedAccount<'info>,
    #[account(mut)]
    b: UncheckedAccount<'info>,
}

// Same struct, but `a` is explicitly permitted to alias `b`: the pair is opted out.
#[derive(VerifiedAccounts)]
struct DupMutAllowed<'info> {
    #[account(mut, allow_duplicate = b)]
    a: UncheckedAccount<'info>,
    #[account(mut)]
    b: UncheckedAccount<'info>,
}

// A `mut` field paired with a NON-mut field: no distinct-key obligation (only mut pairs).
#[derive(VerifiedAccounts)]
struct OneMut<'info> {
    #[account(mut)]
    a: UncheckedAccount<'info>,
    b: UncheckedAccount<'info>,
}

/// A writable account at a chosen key.
fn writable_at(key: Pubkey) -> Acct {
    Acct { key, owner: Pubkey::new_unique(), lamports: 1, data: vec![], is_signer: false, is_writable: true }
}

#[test]
fn dup_mut_accepts_distinct_keys() {
    let mut a = writable_at(Pubkey::new_unique());
    let mut b = writable_at(Pubkey::new_unique());
    let accts = [a.info(), b.info()];
    assert_eq!(DupMut::validate(&accts, &[], &any_pid()), Ok(()));
}

#[test]
fn dup_mut_rejects_same_key() {
    let dup = Pubkey::new_unique();
    let mut a = writable_at(dup);
    let mut b = writable_at(dup);
    let accts = [a.info(), b.info()];
    assert_eq!(DupMut::validate(&accts, &[], &any_pid()),
               Err(VAError::DuplicateAccount { field_a: "a", field_b: "b" }));
}

#[test]
fn dup_mut_opt_out_allows_same_key() {
    let dup = Pubkey::new_unique();
    let mut a = writable_at(dup);
    let mut b = writable_at(dup);
    let accts = [a.info(), b.info()];
    // The explicit `allow_duplicate = b` opt-out lets the collision through.
    assert_eq!(DupMutAllowed::validate(&accts, &[], &any_pid()), Ok(()));
}

#[test]
fn one_mut_pair_has_no_distinct_obligation() {
    // a (mut) and b (read-only) share a key — only mut/mut pairs are checked, so this is fine.
    let dup = Pubkey::new_unique();
    let mut a = writable_at(dup);
    let mut b = Acct { key: dup, owner: Pubkey::new_unique(), lamports: 1, data: vec![],
                       is_signer: false, is_writable: false };
    let accts = [a.info(), b.info()];
    assert_eq!(OneMut::validate(&accts, &[], &any_pid()), Ok(()));
}

// ---- M9 Task 8: `zero` validate check ----
//
// `zero` checks that the first 8 bytes of the account data are all-zero (reinit guard).
// An account whose first 8 bytes are [0u8; 8] is accepted; any non-zero byte is rejected
// with `VAError::NotZeroed`.

#[derive(VerifiedAccounts)]
struct ZeroGuard<'info> {
    #[account(zero)]
    uninit: UncheckedAccount<'info>,
}

fn acct_with_zeros() -> Acct {
    // 8 zero bytes — accepted by the `zero` constraint
    Acct { key: Pubkey::new_unique(), owner: Pubkey::new_unique(), lamports: 1,
           data: vec![0u8; 8], is_signer: false, is_writable: false }
}

fn acct_with_nonzero_disc() -> Acct {
    // First byte non-zero — rejected by the `zero` constraint
    Acct { key: Pubkey::new_unique(), owner: Pubkey::new_unique(), lamports: 1,
           data: vec![1u8, 0, 0, 0, 0, 0, 0, 0], is_signer: false, is_writable: false }
}

#[test]
fn zero_accepts_all_zero_discriminator() {
    let mut a = acct_with_zeros();
    let accts = [a.info()];
    assert_eq!(ZeroGuard::validate(&accts, &[], &any_pid()), Ok(()));
}

#[test]
fn zero_rejects_non_zero_discriminator() {
    let mut a = acct_with_nonzero_disc();
    let accts = [a.info()];
    assert_eq!(
        ZeroGuard::validate(&accts, &[], &any_pid()),
        Err(VAError::NotZeroed { field: "uninit" })
    );
}

#[test]
fn zero_rejects_short_data() {
    // Fewer than 8 bytes → also rejected (can't confirm all-zero prefix of length 8)
    let mut a = Acct { key: Pubkey::new_unique(), owner: Pubkey::new_unique(), lamports: 1,
                       data: vec![0u8; 4], is_signer: false, is_writable: false };
    let accts = [a.info()];
    assert_eq!(
        ZeroGuard::validate(&accts, &[], &any_pid()),
        Err(VAError::NotZeroed { field: "uninit" })
    );
}

// ---- M10 Task 5: `AccountData` carries the layout ----
//
// `LAYOUT` is the runtime Borsh descriptor the generated locator walks; `LAYOUT_LEAN` is the
// same descriptor as Lean `Ty` source, spliced into `lean_spec()` at runtime.

#[test]
fn account_data_derive_emits_real_layout() {
    use verified_anchor::layout::{locate, Ty};

    #[derive(
        verified_anchor::borsh::BorshSerialize,
        verified_anchor::borsh::BorshDeserialize,
        verified_anchor::AccountData,
    )]
    #[borsh(crate = "::verified_anchor::borsh")]
    struct LayoutProbe {
        bump: u8,
        authority: solana_program::pubkey::Pubkey,
    }

    // The descriptor names the real fields in declaration order.
    match <LayoutProbe as verified_anchor::AccountData>::LAYOUT {
        Ty::Struct(fs) => {
            assert_eq!(fs.len(), 2);
            assert_eq!(fs[0].0, "bump");
            assert_eq!(fs[1].0, "authority");
            assert_eq!(fs[1].1, Ty::Pubkey);
        }
        other => panic!("expected a struct descriptor, got {other:?}"),
    }

    // authority sits at offset 1 within the struct body, NOT offset 0.
    let data = vec![0u8; 33];
    let ty = <LayoutProbe as verified_anchor::AccountData>::LAYOUT;
    assert_eq!(locate(&ty, &["authority"], &data, 0).map(|r| r.0), Some(1));

    assert_eq!(
        <LayoutProbe as verified_anchor::AccountData>::LAYOUT_LEAN,
        "(Ty.struct [(\"bump\", Ty.u8), (\"authority\", Ty.pubkey)])"
    );
}

// ---- M10 Task 7: `has_one` reads the NAMED field, not byte 8 ----
//
// REGRESSION for the v0.3.0 defect: the generated check hardcoded `&data[8..40]`, so
// `has_one = authority` compared the FIRST field of the account struct whatever field was
// named. Here `authority` is the SECOND field, so it lives at offset 9 (8 discriminator + 1
// byte of `bump`); a hardcoded-8 read splices one byte of `bump` onto 31 bytes of the key.

#[derive(
    verified_anchor::borsh::BorshSerialize,
    verified_anchor::borsh::BorshDeserialize,
    verified_anchor::AccountData,
)]
#[borsh(crate = "::verified_anchor::borsh")]
struct OffsetVault {
    bump: u8,
    authority: Pubkey,
}

#[derive(VerifiedAccounts)]
struct CheckOffsetHasOne<'info> {
    #[account(has_one = authority)]
    vault: verified_anchor::Account<'info, OffsetVault>,
    authority: UncheckedAccount<'info>,
}

fn offset_vault_data(bump: u8, authority: Pubkey) -> Vec<u8> {
    let mut d = <OffsetVault as verified_anchor::AccountData>::DISCRIMINATOR.to_vec();
    d.push(bump);
    d.extend_from_slice(authority.as_ref());
    d
}

#[test]
fn has_one_accepts_target_at_nonzero_offset() {
    let auth = Pubkey::new_unique();
    let mut v = acct_with_data(Pubkey::new_unique(), offset_vault_data(254, auth));
    // `Account<'info, T>` implies `owner = crate::ID`, so the fixture must be program-owned
    // for the has_one check to even be reached.
    v.owner = crate::ID;
    let mut a = acct_with_data(auth, vec![]);
    let accts = [v.info(), a.info()];
    assert_eq!(CheckOffsetHasOne::validate(&accts, &[], &any_pid()), Ok(()));
}

#[test]
fn has_one_rejects_mismatch_at_nonzero_offset() {
    let auth = Pubkey::new_unique();
    let mut v = acct_with_data(Pubkey::new_unique(), offset_vault_data(254, auth));
    v.owner = crate::ID;
    let mut a = acct_with_data(Pubkey::new_unique(), vec![]);   // different key
    let accts = [v.info(), a.info()];
    assert_eq!(
        CheckOffsetHasOne::validate(&accts, &[], &any_pid()),
        Err(VAError::WrongHasOne { field: "vault", target: "authority" })
    );
}

#[test]
fn lean_spec_carries_the_real_layout() {
    let s = CheckOffsetHasOne::lean_spec();
    assert!(s.contains("(\"bump\", Ty.u8)"), "spec was: {s}");
    assert!(s.contains("(\"authority\", Ty.pubkey)"), "spec was: {s}");
    assert!(!s.contains("(\"authority\", 8)"), "spec still hardcodes offset 8: {s}");
}

/// The build-time `has_one` guard keys off the field NAME being present in the descriptor, not
/// off the layout being fixed-width. A variable-width earlier field is perfectly locatable at
/// runtime (its width comes from its length prefix), so this must still compile AND validate —
/// i.e. the guard added for the truncated-descriptor case must not over-reject.
#[verified_anchor::account]
struct LabelVault {
    label: String,
    authority: Pubkey,
}

fn label_vault_data(label: &str, authority: Pubkey) -> Vec<u8> {
    let mut d = <LabelVault as verified_anchor::AccountData>::DISCRIMINATOR.to_vec();
    d.extend_from_slice(&(label.len() as u32).to_le_bytes());   // borsh String length prefix
    d.extend_from_slice(label.as_bytes());
    d.extend_from_slice(authority.as_ref());
    d
}

#[derive(VerifiedAccounts)]
struct CheckLabelHasOne<'info> {
    #[account(has_one = authority)]
    vault: verified_anchor::Account<'info, LabelVault>,
    authority: UncheckedAccount<'info>,
}

#[test]
fn has_one_locates_past_a_variable_width_field() {
    let auth = Pubkey::new_unique();
    let mut v = acct_with_data(Pubkey::new_unique(), label_vault_data("a-long-ish-label", auth));
    v.owner = crate::ID;
    let mut a = acct_with_data(auth, vec![]);
    let accts = [v.info(), a.info()];
    assert_eq!(CheckLabelHasOne::validate(&accts, &[], &any_pid()), Ok(()));
}

#[test]
fn has_one_rejects_mismatch_past_a_variable_width_field() {
    let mut v = acct_with_data(
        Pubkey::new_unique(),
        label_vault_data("a-long-ish-label", Pubkey::new_unique()),
    );
    v.owner = crate::ID;
    let mut a = acct_with_data(Pubkey::new_unique(), vec![]);
    let accts = [v.info(), a.info()];
    assert_eq!(
        CheckLabelHasOne::validate(&accts, &[], &any_pid()),
        Err(VAError::WrongHasOne { field: "vault", target: "authority" })
    );
}

// ── M10 Task 9: `#[instruction(...)]` args and Anchor-shaped `name.as_bytes()` seeds ──────
//
// This is the drop-in target: the struct below is REAL, UNMODIFIED Anchor source. Before
// M10 the same PDA required `seeds = [b"vault", arg(4, 5)]` — bespoke syntax no Anchor
// program contains, and one that hardcodes the argument's length.

// NOTE the attribute ORDER: `#[derive(..)]` first, `#[instruction(..)]` second. `instruction`
// is a DERIVE HELPER (exactly as in Anchor, whose derive is
// `#[proc_macro_derive(Accounts, attributes(account, instruction))]`), and rustc rejects a
// helper written before the derive that introduces it (`legacy_derive_helpers`, deny-by-default).
// Canonical Anchor source is therefore already in this order.
#[derive(VerifiedAccounts)]
#[instruction(name: String)]
struct SeedFromArg<'info> {
    #[account(seeds = [b"vault", name.as_bytes()], bump)]
    pda: UncheckedAccount<'info>,
}

/// Borsh-encode a single `String` argument the way a client would: 4-byte LE length prefix
/// then the payload. `instr_data` is the argument buffer with any discriminator ALREADY
/// stripped by the caller (what Anchor hands `try_accounts`), so decoding starts at 0.
fn borsh_string_arg(s: &str) -> Vec<u8> {
    let mut v = (s.len() as u32).to_le_bytes().to_vec();
    v.extend_from_slice(s.as_bytes());
    v
}

#[test]
fn seeds_resolve_a_named_string_arg() {
    let pid = any_pid();
    let (expected, _bump) = Pubkey::find_program_address(&[b"vault", b"alice"], &pid);
    let mut p = acct_with_data(expected, vec![]);
    let accts = [p.info()];
    assert_eq!(SeedFromArg::validate(&accts, &borsh_string_arg("alice"), &pid), Ok(()));
}

#[test]
fn seeds_reject_a_wrong_named_string_arg() {
    let pid = any_pid();
    let (expected, _bump) = Pubkey::find_program_address(&[b"vault", b"alice"], &pid);
    let mut p = acct_with_data(expected, vec![]);
    let accts = [p.info()];
    assert_eq!(
        SeedFromArg::validate(&accts, &borsh_string_arg("mallory"), &pid),
        Err(VAError::WrongPda { field: "pda" })
    );
}

// ── M10 Task 9: `argBytes` parity with the Lean model ─────────────────────────────────────
//
// `AccountsStruct.argBytes` (lean/VerifiedAnchor/Constraints/Context.lean) is the contract the
// macro's `name.as_bytes()` codegen mirrors. A divergence here would make verified-anchor
// derive a DIFFERENT PDA than real Anchor: our own tests would still pass (they would agree
// with our own model) and the defect would surface only in production, as an address mismatch.
//
// LEAN_ARG_FIXTURE is copied verbatim from the Lean regression fixture `argCtx` in
// `lean/VerifiedAnchor/Codegen/ExampleGenerated.lean`, whose `by decide` proofs pin:
//     argBytes "amount"  = #[1,0,0,0,0,0,0,0]   -- u64: the WHOLE fixed-size encoding
//     argBytes "label"   = #[104, 105]          -- string: LENGTH PREFIX STRIPPED
//     argBytes "missing" = none
// The tests below assert the PDA our generated code accepts equals the PDA derived from those
// exact raw byte strings — so they compare SEED BYTES, not merely the accept/reject verdict.
const LEAN_ARG_FIXTURE: [u8; 14] = [1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 104, 105];

// `amount.to_le_bytes()` is exactly what a real Anchor program writes for a numeric seed, and
// `argBytes`' fixed-size arm returns exactly that little-endian encoding.
#[derive(VerifiedAccounts)]
#[instruction(amount: u64, label: String)]
struct ArgFixedSeed<'info> {
    #[account(seeds = [b"vault", amount.to_le_bytes()], bump)]
    pda: UncheckedAccount<'info>,
}

#[derive(VerifiedAccounts)]
#[instruction(amount: u64, label: String)]
struct ArgStringSeed<'info> {
    #[account(seeds = [b"vault", label.as_bytes()], bump)]
    pda: UncheckedAccount<'info>,
}

#[derive(VerifiedAccounts)]
#[instruction(blob: Vec<u8>)]
struct ArgVecSeed<'info> {
    #[account(seeds = [b"vault", blob.as_bytes()], bump)]
    pda: UncheckedAccount<'info>,
}

/// Fixed-size arm: `argBytes` returns the whole 8-byte Borsh encoding, no framing to strip.
#[test]
fn arg_bytes_fixed_size_uses_the_whole_encoding() {
    let pid = any_pid();
    let (expected, _) = Pubkey::find_program_address(&[b"vault", &[1, 0, 0, 0, 0, 0, 0, 0]], &pid);
    let mut p = acct_with_data(expected, vec![]);
    let accts = [p.info()];
    assert_eq!(ArgFixedSeed::validate(&accts, &LEAN_ARG_FIXTURE, &pid), Ok(()));
}

/// THE POINT, and the top failure mode of this task. `label` is at offset 8 (after the u64),
/// its raw Borsh encoding is the 6 bytes `[2,0,0,0,104,105]`, and `argBytes` — like Anchor's
/// `label.as_bytes()` — yields only the 2 payload bytes `[104,105]`.
#[test]
fn arg_bytes_string_strips_the_length_prefix() {
    let pid = any_pid();
    let (payload_only, _) = Pubkey::find_program_address(&[b"vault", &[104, 105]], &pid);
    let mut p = acct_with_data(payload_only, vec![]);
    let accts = [p.info()];
    assert_eq!(ArgStringSeed::validate(&accts, &LEAN_ARG_FIXTURE, &pid), Ok(()));
}

/// The negative half of the parity claim: had we kept the Borsh framing (the natural mistake),
/// we would derive THIS address instead. It must be rejected, and it must differ from the one
/// accepted above — otherwise the assertion above proves nothing about prefix stripping.
#[test]
fn arg_bytes_string_does_not_include_the_borsh_framing() {
    let pid = any_pid();
    let (payload_only, _) = Pubkey::find_program_address(&[b"vault", &[104, 105]], &pid);
    let (with_framing, _) =
        Pubkey::find_program_address(&[b"vault", &[2, 0, 0, 0, 104, 105]], &pid);
    assert_ne!(payload_only, with_framing);
    let mut p = acct_with_data(with_framing, vec![]);
    let accts = [p.info()];
    assert_eq!(
        ArgStringSeed::validate(&accts, &LEAN_ARG_FIXTURE, &pid),
        Err(VAError::WrongPda { field: "pda" })
    );
}

/// `vec` takes the same length-prefix-stripping arm as `string` in Lean `argBytes`.
#[test]
fn arg_bytes_vec_strips_the_length_prefix() {
    let pid = any_pid();
    let (expected, _) = Pubkey::find_program_address(&[b"vault", &[9u8, 8, 7]], &pid);
    let mut p = acct_with_data(expected, vec![]);
    let accts = [p.info()];
    let data = [3u8, 0, 0, 0, 9, 8, 7];
    assert_eq!(ArgVecSeed::validate(&accts, &data, &pid), Ok(()));
}

/// Fail-closed, both arms: Lean `argBytes` returns `none` when the declared argument overruns
/// the buffer, and `none` cannot produce a matching PDA. Rust must reject, never panic.
#[test]
fn arg_bytes_overrun_fails_closed() {
    let pid = any_pid();
    let (expected, _) = Pubkey::find_program_address(&[b"vault", b"alice"], &pid);
    let mut p = acct_with_data(expected, vec![]);
    let accts = [p.info()];
    // Empty buffer: not even the u32 length prefix is present.
    assert_eq!(
        SeedFromArg::validate(&accts, &[], &pid),
        Err(VAError::WrongPda { field: "pda" })
    );
    // Length prefix claims 5 payload bytes; only 2 follow.
    assert_eq!(
        SeedFromArg::validate(&accts, &[5, 0, 0, 0, 97, 108], &pid),
        Err(VAError::WrongPda { field: "pda" })
    );
    // Truncated fixed-size argument (u64 needs 8 bytes, 3 given).
    assert_eq!(
        ArgFixedSeed::validate(&accts, &[1, 0, 0], &pid),
        Err(VAError::WrongPda { field: "pda" })
    );
}

/// A zero-length string yields an EMPTY seed, not a failure — Lean's `off + 4 + 0 ≤ size`
/// holds, so `argBytes` returns the empty ByteArray.
#[test]
fn arg_bytes_empty_string_is_an_empty_seed() {
    let pid = any_pid();
    let (expected, _) = Pubkey::find_program_address(&[b"vault", b""], &pid);
    let mut p = acct_with_data(expected, vec![]);
    let accts = [p.info()];
    assert_eq!(SeedFromArg::validate(&accts, &[0, 0, 0, 0], &pid), Ok(()));
}

/// The `Bumps` struct is built by a SECOND copy of the seed codegen inside `try_accounts`.
/// It must resolve `name.as_bytes()` the same way `validate` does, or the canonical bump handed
/// back to the handler would be derived from different seeds than the ones just validated.
#[test]
fn bumps_struct_resolves_a_named_arg_seed() {
    use verified_anchor::Accounts;
    let pid = any_pid();
    let (expected, expected_bump) = Pubkey::find_program_address(&[b"vault", b"alice"], &pid);
    let mut p = acct_with_data(expected, vec![]);
    let accts = [p.info()];
    let (_s, bumps) =
        <SeedFromArg as Accounts>::try_accounts(&pid, &accts, &borsh_string_arg("alice")).unwrap();
    assert_eq!(bumps.pda, expected_bump);
}

// ── M10 Task 9: numeric seeds via `to_le_bytes()` ─────────────────────────────────────────
//
// The canonical Anchor spelling for a numeric PDA seed. `.as_ref()` is what real source writes
// (a bare `[u8; 8]` does not coerce to `&[u8]` inside a seed list); it carries no meaning here
// and is peeled, as is a leading `&`.

#[derive(VerifiedAccounts)]
#[instruction(amount: u64)]
struct Deposit<'info> {
    #[account(seeds = [b"vault", amount.to_le_bytes().as_ref()], bump)]
    vault: UncheckedAccount<'info>,
}

#[derive(VerifiedAccounts)]
#[instruction(amount: u64)]
struct DepositRef<'info> {
    #[account(seeds = [b"vault", &amount.to_le_bytes()], bump)]
    vault: UncheckedAccount<'info>,
}

#[derive(VerifiedAccounts)]
#[instruction(idx: u16, amount: u64)]
struct DepositAtOffset<'info> {
    #[account(seeds = [b"vault", amount.to_le_bytes().as_ref()], bump)]
    vault: UncheckedAccount<'info>,
}

/// The address must be the one REAL ANCHOR would derive: built here straight from
/// `amount.to_le_bytes()`, with no reference to our own decoding path.
#[test]
fn numeric_arg_seed_derives_the_anchor_address() {
    let pid = any_pid();
    let amount: u64 = 42;
    let (expected, _) = Pubkey::find_program_address(&[b"vault", amount.to_le_bytes().as_ref()], &pid);
    let mut p = acct_with_data(expected, vec![]);
    let accts = [p.info()];
    let data = amount.to_le_bytes().to_vec();
    assert_eq!(Deposit::validate(&accts, &data, &pid), Ok(()));
}

/// `&amount.to_le_bytes()` is the same seed as `amount.to_le_bytes().as_ref()`.
#[test]
fn numeric_arg_seed_accepts_the_reference_spelling() {
    let pid = any_pid();
    let amount: u64 = 42;
    let (expected, _) = Pubkey::find_program_address(&[b"vault", amount.to_le_bytes().as_ref()], &pid);
    let mut p = acct_with_data(expected, vec![]);
    let accts = [p.info()];
    assert_eq!(DepositRef::validate(&accts, &amount.to_le_bytes(), &pid), Ok(()));
}

/// Negative: a different amount is a different PDA. Without this the test above would pass for
/// an implementation that ignored the argument entirely.
#[test]
fn numeric_arg_seed_rejects_a_different_amount() {
    let pid = any_pid();
    let (expected, _) = Pubkey::find_program_address(&[b"vault", 42u64.to_le_bytes().as_ref()], &pid);
    let mut p = acct_with_data(expected, vec![]);
    let accts = [p.info()];
    assert_eq!(
        Deposit::validate(&accts, &43u64.to_le_bytes(), &pid),
        Err(VAError::WrongPda { field: "vault" })
    );
}

/// Big-endian is the failure this guard exists for: had `to_be_bytes()` been silently accepted,
/// THIS is the address it would have derived. It must differ, and must be rejected.
#[test]
fn numeric_arg_seed_is_little_endian() {
    let pid = any_pid();
    let amount: u64 = 42;
    let (le, _) = Pubkey::find_program_address(&[b"vault", amount.to_le_bytes().as_ref()], &pid);
    let (be, _) = Pubkey::find_program_address(&[b"vault", amount.to_be_bytes().as_ref()], &pid);
    assert_ne!(le, be, "the fixture must distinguish the two endiannesses");
    let mut p = acct_with_data(be, vec![]);
    let accts = [p.info()];
    assert_eq!(
        Deposit::validate(&accts, &amount.to_le_bytes(), &pid),
        Err(VAError::WrongPda { field: "vault" })
    );
}

/// A numeric seed behind another argument: the offset comes from Borsh, not from the seed list.
#[test]
fn numeric_arg_seed_at_a_nonzero_offset() {
    let pid = any_pid();
    let amount: u64 = 7;
    let (expected, _) = Pubkey::find_program_address(&[b"vault", amount.to_le_bytes().as_ref()], &pid);
    let mut p = acct_with_data(expected, vec![]);
    let accts = [p.info()];
    let mut data = 3u16.to_le_bytes().to_vec();   // idx, declared first
    data.extend_from_slice(&amount.to_le_bytes());
    assert_eq!(DepositAtOffset::validate(&accts, &data, &pid), Ok(()));
}

/// Fail-closed on a truncated numeric argument, same as the string arm.
#[test]
fn numeric_arg_seed_overrun_fails_closed() {
    let pid = any_pid();
    let (expected, _) = Pubkey::find_program_address(&[b"vault", 42u64.to_le_bytes().as_ref()], &pid);
    let mut p = acct_with_data(expected, vec![]);
    let accts = [p.info()];
    assert_eq!(
        Deposit::validate(&accts, &[42, 0, 0], &pid),
        Err(VAError::WrongPda { field: "vault" })
    );
}

// ── M10 Task 9: the remaining Anchor seed spellings ───────────────────────────────────────
//
// `&x` / `.as_ref()` / `.as_slice()` are slice-coercion noise around a seed source: a seed list
// is `&[&[u8]]`, and `Pubkey`, `[u8; 8]` and `Vec<u8>` do not coerce to `&[u8]` there. They are
// peeled, then the bare name is resolved — INSTRUCTION ARGUMENTS FIRST, then account fields.

#[derive(VerifiedAccounts)]
struct KeyAsRefSeed<'info> {
    #[account(seeds = [b"vault", user.key().as_ref()], bump)]
    pda: UncheckedAccount<'info>,
    user: UncheckedAccount<'info>,
}

#[derive(VerifiedAccounts)]
#[instruction(authority: Pubkey)]
struct PubkeyArgSeed<'info> {
    #[account(seeds = [b"vault", authority.as_ref()], bump)]
    pda: UncheckedAccount<'info>,
}

#[derive(VerifiedAccounts)]
#[instruction(blob: Vec<u8>)]
struct VecArgRefSeed<'info> {
    #[account(seeds = [b"vault", &blob], bump)]
    pda: UncheckedAccount<'info>,
}

#[derive(VerifiedAccounts)]
#[instruction(blob: Vec<u8>)]
struct VecArgSliceSeed<'info> {
    #[account(seeds = [b"vault", blob.as_slice()], bump)]
    pda: UncheckedAccount<'info>,
}

/// `user.key().as_ref()` — the ubiquitous Anchor account-key seed. Expected address built
/// straight from the account's key bytes.
#[test]
fn account_key_as_ref_seed_derives_the_anchor_address() {
    let pid = any_pid();
    let user_key = Pubkey::new_unique();
    let (expected, _) = Pubkey::find_program_address(&[b"vault", user_key.as_ref()], &pid);
    let mut p = acct_with_data(expected, vec![]);
    let mut u = acct_with_data(user_key, vec![]);
    let accts = [p.info(), u.info()];
    assert_eq!(KeyAsRefSeed::validate(&accts, &[], &pid), Ok(()));
}

#[test]
fn account_key_as_ref_seed_rejects_a_different_key() {
    let pid = any_pid();
    let (expected, _) = Pubkey::find_program_address(&[b"vault", Pubkey::new_unique().as_ref()], &pid);
    let mut p = acct_with_data(expected, vec![]);
    let mut u = acct_with_data(Pubkey::new_unique(), vec![]);   // a DIFFERENT user account
    let accts = [p.info(), u.info()];
    assert_eq!(
        KeyAsRefSeed::validate(&accts, &[], &pid),
        Err(VAError::WrongPda { field: "pda" })
    );
}

/// A `Pubkey` INSTRUCTION ARGUMENT via `.as_ref()`. `argBytes`' fixed-size arm returns the whole
/// 32-byte encoding, which is exactly `Pubkey::as_ref()`.
#[test]
fn pubkey_arg_seed_derives_the_anchor_address() {
    let pid = any_pid();
    let authority = Pubkey::new_unique();
    let (expected, _) = Pubkey::find_program_address(&[b"vault", authority.as_ref()], &pid);
    let mut p = acct_with_data(expected, vec![]);
    let accts = [p.info()];
    assert_eq!(PubkeyArgSeed::validate(&accts, authority.as_ref(), &pid), Ok(()));
}

#[test]
fn pubkey_arg_seed_rejects_a_different_authority() {
    let pid = any_pid();
    let (expected, _) = Pubkey::find_program_address(&[b"vault", Pubkey::new_unique().as_ref()], &pid);
    let mut p = acct_with_data(expected, vec![]);
    let accts = [p.info()];
    assert_eq!(
        PubkeyArgSeed::validate(&accts, Pubkey::new_unique().as_ref(), &pid),
        Err(VAError::WrongPda { field: "pda" })
    );
    // ...and a truncated Pubkey argument fails closed rather than panicking.
    assert_eq!(
        PubkeyArgSeed::validate(&accts, &[7u8; 31], &pid),
        Err(VAError::WrongPda { field: "pda" })
    );
}

/// `&blob` and `blob.as_slice()` are the same seed: the `Vec` payload, length prefix stripped.
#[test]
fn vec_arg_reference_and_slice_spellings_agree() {
    let pid = any_pid();
    let (expected, _) = Pubkey::find_program_address(&[b"vault", &[9u8, 8, 7]], &pid);
    let mut p = acct_with_data(expected, vec![]);
    let accts = [p.info()];
    let data = [3u8, 0, 0, 0, 9, 8, 7];
    assert_eq!(VecArgRefSeed::validate(&accts, &data, &pid), Ok(()));
    assert_eq!(VecArgSliceSeed::validate(&accts, &data, &pid), Ok(()));
    // Negative: a different payload is a different PDA.
    assert_eq!(
        VecArgRefSeed::validate(&accts, &[3u8, 0, 0, 0, 9, 8, 6], &pid),
        Err(VAError::WrongPda { field: "pda" })
    );
}

/// Literal seeds take the same peeling: `b"vault".as_ref()`, `&b"vault"` and `"vault".as_bytes()`
/// are all the bare `b"vault"` seed. Anchor programs use these interchangeably.
#[derive(VerifiedAccounts)]
struct LiteralSeedSpellings<'info> {
    #[account(seeds = [b"vault".as_ref(), &b"x", "tail".as_bytes()], bump)]
    pda: UncheckedAccount<'info>,
}

#[test]
fn literal_seed_spellings_all_derive_the_same_address() {
    let pid = any_pid();
    let (expected, _) = Pubkey::find_program_address(&[b"vault", b"x", b"tail"], &pid);
    let mut p = acct_with_data(expected, vec![]);
    let accts = [p.info()];
    assert_eq!(LiteralSeedSpellings::validate(&accts, &[], &pid), Ok(()));
    // Negative: a different literal ordering is a different PDA.
    let (other, _) = Pubkey::find_program_address(&[b"tail", b"x", b"vault"], &pid);
    assert_ne!(expected, other);
}

// ── M10 Task 12: `constraint = <expr>` compiled into the proven sublanguage ────────────────

#[derive(
    verified_anchor::borsh::BorshSerialize,
    verified_anchor::borsh::BorshDeserialize,
    verified_anchor::AccountData,
)]
#[borsh(crate = "::verified_anchor::borsh")]
struct ExprVault {
    bump: u8,
    amount: u64,
}

#[derive(VerifiedAccounts)]
struct CheckExpr<'info> {
    #[account(constraint = vault.amount >= 1000)]
    vault: verified_anchor::Account<'info, ExprVault>,
    user: UncheckedAccount<'info>,
}

fn expr_vault_data(bump: u8, amount: u64) -> Vec<u8> {
    let mut d = <ExprVault as verified_anchor::AccountData>::DISCRIMINATOR.to_vec();
    d.push(bump);
    d.extend_from_slice(&amount.to_le_bytes());
    d
}

#[test]
fn constraint_expr_accepts_when_satisfied() {
    let mut v = acct_with_data(Pubkey::new_unique(), expr_vault_data(1, 1000));
    // `Account<'info, T>` implies `owner = crate::ID`; without it validate stops before the expr.
    v.owner = crate::ID;
    let mut u = acct_with_data(Pubkey::new_unique(), vec![]);
    let accts = [v.info(), u.info()];
    assert_eq!(CheckExpr::validate(&accts, &[], &any_pid()), Ok(()));
}

#[test]
fn constraint_expr_rejects_when_violated() {
    let mut v = acct_with_data(Pubkey::new_unique(), expr_vault_data(1, 999));
    // `Account<'info, T>` implies `owner = crate::ID`; without it validate stops before the expr.
    v.owner = crate::ID;
    let mut u = acct_with_data(Pubkey::new_unique(), vec![]);
    let accts = [v.info(), u.info()];
    assert!(matches!(
        CheckExpr::validate(&accts, &[], &any_pid()),
        Err(VAError::ConstraintViolated { field: "vault", .. })
    ));
}

#[derive(VerifiedAccounts)]
struct CheckKeyExpr<'info> {
    #[account(constraint = a.key() != b.key())]
    a: UncheckedAccount<'info>,
    b: UncheckedAccount<'info>,
}

#[test]
fn constraint_expr_compares_keys() {
    let mut a = acct_with_data(Pubkey::new_unique(), vec![]);
    let mut b = acct_with_data(Pubkey::new_unique(), vec![]);
    assert_eq!(CheckKeyExpr::validate(&[a.info(), b.info()], &[], &any_pid()), Ok(()));

    let k = Pubkey::new_unique();
    let mut c = acct_with_data(k, vec![]);
    let mut d = acct_with_data(k, vec![]);
    assert!(CheckKeyExpr::validate(&[c.info(), d.info()], &[], &any_pid()).is_err());
}

// ── STRICTNESS: `&&`/`||` must NOT lower to Rust's short-circuiting operators ──────────────
//
// Lean's `evalExpr` binds BOTH operands through the `Option` monad before combining, so an
// unevaluable operand poisons the whole expression regardless of the other side. `key() < 1`
// is unevaluable BY CONSTRUCTION (`evalCmp` has no ordering arm for `key`/`nat`, deliberately —
// see the type-confusion guarantee in `Codegen/ExampleGenerated.lean`), so these fixtures pin
// the semantics without depending on any particular account bytes.

#[derive(VerifiedAccounts)]
struct StrictOr<'info> {
    // `true || <unevaluable>`. Under Rust's native `||` this SHORT-CIRCUITS to `true` and the
    // account set is ACCEPTED — while the Lean contract says `none`, i.e. REJECT. That gap is
    // precisely "verified-anchor accepts an account set the contract rejects", the milestone's
    // headline guarantee. If a future refactor swaps the strict combinator for native `||`,
    // `strict_or_rejects_when_right_operand_is_unevaluable` below turns red.
    #[account(constraint = user.is_signer || user.key() < 1)]
    user: UncheckedAccount<'info>,
}

#[test]
fn strict_or_rejects_when_right_operand_is_unevaluable() {
    let mut u = acct(true, false); // is_signer = true → the left operand is `Some(true)`
    let accts = [u.info()];
    assert!(
        matches!(
            StrictOr::validate(&accts, &[], &any_pid()),
            Err(VAError::ConstraintViolated { field: "user", .. })
        ),
        "`true || <unevaluable>` MUST reject: Lean's `evalExpr` yields `none`, not `some true`. \
         Native Rust `||` would short-circuit and accept."
    );
}

#[derive(VerifiedAccounts)]
struct StrictAnd<'info> {
    // `false && <unevaluable>`: rejects under both semantics, so this is not a safety
    // difference — it is here so the `and` arm is exercised at all, and so the pair documents
    // exactly where the asymmetry lies.
    #[account(constraint = user.is_signer && user.key() < 1)]
    user: UncheckedAccount<'info>,
}

#[test]
fn strict_and_rejects_when_an_operand_is_unevaluable() {
    let mut u = acct(false, false);
    let accts = [u.info()];
    assert!(StrictAnd::validate(&accts, &[], &any_pid()).is_err());
}

#[derive(VerifiedAccounts)]
struct EvaluableOr<'info> {
    // The control: when BOTH operands evaluate, `or` behaves like ordinary disjunction. Without
    // this, "reject everything" would pass the two strictness tests above.
    #[account(constraint = a.is_signer || b.is_signer)]
    a: UncheckedAccount<'info>,
    b: UncheckedAccount<'info>,
}

#[test]
fn evaluable_or_still_accepts_and_rejects_normally() {
    let mut a = acct(false, false);
    let mut b = acct(true, false);
    assert_eq!(EvaluableOr::validate(&[a.info(), b.info()], &[], &any_pid()), Ok(()));
    let mut c = acct(false, false);
    let mut d = acct(false, false);
    assert!(EvaluableOr::validate(&[c.info(), d.info()], &[], &any_pid()).is_err());
}

#[derive(VerifiedAccounts)]
struct NotExpr<'info> {
    #[account(constraint = !user.is_writable)]
    user: UncheckedAccount<'info>,
}

#[test]
fn not_expr_negates() {
    let mut r = acct(false, false);
    assert_eq!(NotExpr::validate(&[r.info()], &[], &any_pid()), Ok(()));
    let mut w = acct(false, true);
    assert!(NotExpr::validate(&[w.info()], &[], &any_pid()).is_err());
}

// A bare truthy operand — `#[account(constraint = user.is_signer)]` with no comparison.
#[derive(VerifiedAccounts)]
struct TruthyExpr<'info> {
    #[account(constraint = user.is_signer)]
    user: UncheckedAccount<'info>,
}

#[test]
fn truthy_expr_reads_the_metadata_flag() {
    let mut s = acct(true, false);
    assert_eq!(TruthyExpr::validate(&[s.info()], &[], &any_pid()), Ok(()));
    let mut n = acct(false, false);
    assert!(TruthyExpr::validate(&[n.info()], &[], &any_pid()).is_err());
}

// A `constraint` over a named `#[instruction(...)]` argument. This is the case that forced the
// `INSTR_ARGS` emission gate to widen: before Task 12 the const was emitted only when a SEED
// referenced an argument, so this struct would not have compiled.
#[derive(VerifiedAccounts)]
#[instruction(amount: u64)]
struct ArgExpr<'info> {
    #[account(constraint = vault.amount >= amount)]
    vault: verified_anchor::Account<'info, ExprVault>,
}

#[test]
fn constraint_expr_reads_an_instruction_argument() {
    let mut v = acct_with_data(Pubkey::new_unique(), expr_vault_data(1, 1000));
    // `Account<'info, T>` implies `owner = crate::ID`; without it validate stops before the expr.
    v.owner = crate::ID;
    let accts = [v.info()];
    assert_eq!(ArgExpr::validate(&accts, &1000u64.to_le_bytes(), &any_pid()), Ok(()));
    assert!(ArgExpr::validate(&accts, &1001u64.to_le_bytes(), &any_pid()).is_err());
    // Truncated instruction data: the argument cannot be located → `none` → fail closed.
    assert!(ArgExpr::validate(&accts, &[], &any_pid()).is_err());
}

// An out-of-bounds data read fails CLOSED rather than panicking or reading adjacent bytes.
#[test]
fn constraint_expr_fails_closed_on_truncated_data() {
    let mut short = <ExprVault as verified_anchor::AccountData>::DISCRIMINATOR.to_vec();
    short.push(1); // `bump` present, `amount` truncated away
    let mut v = acct_with_data(Pubkey::new_unique(), short);
    // `Account<'info, T>` implies `owner = crate::ID`; without it validate stops before the expr.
    v.owner = crate::ID;
    let mut u = acct_with_data(Pubkey::new_unique(), vec![]);
    let accts = [v.info(), u.info()];
    assert!(CheckExpr::validate(&accts, &[], &any_pid()).is_err());
}

/// The rejection names the constraint the developer wrote. Pinned because `expr_source` renders
/// TOKENS (stable `Span::source_text()` cannot join a multi-token span and returns only the
/// first token — it once produced a bare `"vault"` here).
#[test]
fn constraint_violation_names_the_expression() {
    let mut v = acct_with_data(Pubkey::new_unique(), expr_vault_data(1, 999));
    v.owner = crate::ID;
    let mut u = acct_with_data(Pubkey::new_unique(), vec![]);
    let accts = [v.info(), u.info()];
    assert_eq!(
        CheckExpr::validate(&accts, &[], &any_pid()),
        Err(VAError::ConstraintViolated { field: "vault", expr: "vault.amount >= 1000" })
    );
}

/// The `Display` arm, and the distinct `ProgramError` custom code.
#[test]
fn constraint_violated_renders_and_maps_to_a_distinct_code() {
    let e = VAError::ConstraintViolated { field: "vault", expr: "vault.amount >= 1000" };
    assert_eq!(e.to_string(), "account `vault` violates constraint `vault.amount >= 1000`");
    assert_eq!(
        solana_program::program_error::ProgramError::from(e),
        solana_program::program_error::ProgramError::Custom(18)
    );
}

/// `a.key() != b.key()` renders through the same path, method calls and all.
#[test]
fn constraint_violation_renders_method_calls() {
    let k = Pubkey::new_unique();
    let mut c = acct_with_data(k, vec![]);
    let mut d = acct_with_data(k, vec![]);
    assert_eq!(
        CheckKeyExpr::validate(&[c.info(), d.info()], &[], &any_pid()),
        Err(VAError::ConstraintViolated { field: "a", expr: "a.key() != b.key()" })
    );
}

// ── M10 Task 12, fix round 1: `nat`/`int` comparisons ─────────────────────────────────────
//
// `evalCmp` used to refuse to ORDER a `nat` against an `int` while `eq`/`ne` stayed total and
// answered from constructor equality. That combination was worse than a refusal:
//
//   * `delta == 0` on an `i64` field was `false` for every `delta`  — a brick;
//   * `delta != 0` was `true`  for every `delta`, zero included     — a TAUTOLOGY, i.e. a guard
//     the developer wrote and the framework silently disabled. It passes every happy-path test
//     and surfaces only as an exploit.
//
// Both Lean and the codegen now compare numerically. These fixtures pin all four shapes.

#[derive(
    verified_anchor::borsh::BorshSerialize,
    verified_anchor::borsh::BorshDeserialize,
    verified_anchor::AccountData,
)]
#[borsh(crate = "::verified_anchor::borsh")]
struct SignedVault {
    delta: i64,
    amount: u64,
}

fn signed_vault_data(delta: i64, amount: u64) -> Vec<u8> {
    let mut d = <SignedVault as verified_anchor::AccountData>::DISCRIMINATOR.to_vec();
    d.extend_from_slice(&delta.to_le_bytes());
    d.extend_from_slice(&amount.to_le_bytes());
    d
}

fn signed_vault(delta: i64, amount: u64) -> Acct {
    let mut a = acct_with_data(Pubkey::new_unique(), signed_vault_data(delta, amount));
    a.owner = crate::ID;
    a
}

// `delta != 0` — the tautology. Must now REJECT when `delta == 0`.
#[derive(VerifiedAccounts)]
struct SignedNeZero<'info> {
    #[account(constraint = vault.delta != 0)]
    vault: verified_anchor::Account<'info, SignedVault>,
}

#[test]
fn signed_ne_unsigned_literal_is_not_a_tautology() {
    let mut zero = signed_vault(0, 0);
    assert_eq!(
        SignedNeZero::validate(&[zero.info()], &[], &any_pid()),
        Err(VAError::ConstraintViolated { field: "vault", expr: "vault.delta != 0" }),
        "`delta != 0` MUST reject a zero delta. Constructor equality made this ACCEPT for every \
         delta — a security check silently disabled."
    );
    // …and still accepts a genuinely non-zero delta, in both signs.
    let mut neg = signed_vault(-1, 0);
    assert_eq!(SignedNeZero::validate(&[neg.info()], &[], &any_pid()), Ok(()));
    let mut pos = signed_vault(7, 0);
    assert_eq!(SignedNeZero::validate(&[pos.info()], &[], &any_pid()), Ok(()));
}

// `delta == 0` — the mirror-image brick. Must now ACCEPT when `delta == 0`.
#[derive(VerifiedAccounts)]
struct SignedEqZero<'info> {
    #[account(constraint = vault.delta == 0)]
    vault: verified_anchor::Account<'info, SignedVault>,
}

#[test]
fn signed_eq_unsigned_literal_is_not_a_brick() {
    let mut zero = signed_vault(0, 0);
    assert_eq!(SignedEqZero::validate(&[zero.info()], &[], &any_pid()), Ok(()));
    let mut neg = signed_vault(-1, 0);
    assert!(SignedEqZero::validate(&[neg.info()], &[], &any_pid()).is_err());
}

// `delta < 0` — mixed ORDERING, which used to be unevaluable and therefore always rejected.
#[derive(VerifiedAccounts)]
struct SignedLtZero<'info> {
    #[account(constraint = vault.delta < 0)]
    vault: verified_anchor::Account<'info, SignedVault>,
}

#[test]
fn signed_ordering_against_an_unsigned_literal_evaluates() {
    let mut neg = signed_vault(-1, 0);
    assert_eq!(SignedLtZero::validate(&[neg.info()], &[], &any_pid()), Ok(()));
    let mut zero = signed_vault(0, 0);
    assert!(SignedLtZero::validate(&[zero.info()], &[], &any_pid()).is_err());
    let mut pos = signed_vault(5, 0);
    assert!(SignedLtZero::validate(&[pos.info()], &[], &any_pid()).is_err());
}

// FIELD vs FIELD across signedness — the case no macro-side literal-type inference could have
// fixed, since neither side is a literal.
#[derive(VerifiedAccounts)]
struct SignedMixedFields<'info> {
    #[account(constraint = vault.delta < vault.amount)]
    vault: verified_anchor::Account<'info, SignedVault>,
}

#[test]
fn mixed_signedness_field_comparison_evaluates() {
    let mut a = signed_vault(-1, 3); // -1 < 3
    assert_eq!(SignedMixedFields::validate(&[a.info()], &[], &any_pid()), Ok(()));
    let mut b = signed_vault(5, 3); // 5 < 3 is false
    assert!(SignedMixedFields::validate(&[b.info()], &[], &any_pid()).is_err());
    // THE BOUNDARY: a u64 whose value exceeds i64 range is still ordered correctly against a
    // negative i64. (`u128`/`i128` in the generated code, but the same widening question.)
    let mut c = signed_vault(-1, u64::MAX);
    assert_eq!(SignedMixedFields::validate(&[c.info()], &[], &any_pid()), Ok(()));
}

// A `u128` field beyond `i128::MAX` — the one place the generated Rust needs a guard Lean does
// not, because `Int` is unbounded there but `u128`/`i128` do not share a range here.
#[derive(
    verified_anchor::borsh::BorshSerialize,
    verified_anchor::borsh::BorshDeserialize,
    verified_anchor::AccountData,
)]
#[borsh(crate = "::verified_anchor::borsh")]
struct WideVault {
    big: u128,
}

#[derive(VerifiedAccounts)]
struct WideGtNeg<'info> {
    #[account(constraint = vault.big > -1)]
    vault: verified_anchor::Account<'info, WideVault>,
}

#[test]
fn unsigned_beyond_i128_max_compares_greater_than_a_negative() {
    let mut d = <WideVault as verified_anchor::AccountData>::DISCRIMINATOR.to_vec();
    // i128::MAX + 1: casting this to i128 would wrap to i128::MIN and invert the comparison.
    d.extend_from_slice(&(i128::MAX as u128 + 1).to_le_bytes());
    let mut a = acct_with_data(Pubkey::new_unique(), d);
    a.owner = crate::ID;
    assert_eq!(WideGtNeg::validate(&[a.info()], &[], &any_pid()), Ok(()));
}

/// The non-numeric pairings are UNCHANGED by the widening: `key < nat` is still unevaluable, so
/// the strictness fixtures above keep testing what they were written to test.
#[test]
fn non_numeric_ordering_still_rejects() {
    let mut u = acct(true, false);
    assert!(StrictOr::validate(&[u.info()], &[], &any_pid()).is_err());
}
