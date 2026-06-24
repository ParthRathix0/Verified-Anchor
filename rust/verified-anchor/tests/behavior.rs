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
