use litesvm::LiteSVM;
use solana_account::Account;
use solana_instruction::{account_meta::AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_message::Message;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;
use std::path::PathBuf;

fn so_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // rust/verified-anchor
    p.pop(); // rust/
    p.push("target/deploy/verified_anchor_program.so");
    p
}

fn system_program_id() -> Pubkey {
    solana_sdk_ids::system_program::id()
}

#[test]
fn init_creates_and_funds_account() {
    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    svm.add_program_from_file(program_id, so_path())
        .expect("load .so (run cargo-build-sbf first)");

    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000).unwrap();
    let new_acct = Keypair::new();

    let ix = Instruction {
        program_id,
        data: vec![0u8],
        accounts: vec![
            AccountMeta::new(new_acct.pubkey(), true),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program_id(), false),
        ],
    };
    let blockhash = svm.latest_blockhash();
    let msg = Message::new(&[ix], Some(&payer.pubkey()));
    let tx = Transaction::new(&[&payer, &new_acct], msg, blockhash);
    svm.send_transaction(tx).expect("init tx succeeds");

    let created = svm
        .get_account(&new_acct.pubkey())
        .expect("account exists after init");
    assert_eq!(created.owner, program_id, "owned by program after init");
    assert!(created.lamports > 0, "account is funded after init");
    assert_eq!(created.data.len(), 8, "8-byte discriminator space (space=0 + 8)");
}

#[test]
fn close_drains_to_dest() {
    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    svm.add_program_from_file(program_id, so_path()).unwrap();

    let target = Pubkey::new_unique();
    let dest = Pubkey::new_unique();

    // Pre-populate target (program-owned, 8-byte data, funded) and dest (empty).
    svm.set_account(
        target,
        Account {
            lamports: 5_000_000,
            data: vec![1u8; 8],
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
    svm.set_account(
        dest,
        Account {
            lamports: 0,
            data: vec![],
            owner: system_program_id(),
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000).unwrap();

    let ix = Instruction {
        program_id,
        data: vec![1u8],
        accounts: vec![
            // target must be writable so the runtime allows lamport mutation
            AccountMeta::new(target, false),
            AccountMeta::new(dest, false),
        ],
    };
    let blockhash = svm.latest_blockhash();
    let msg = Message::new(&[ix], Some(&payer.pubkey()));
    let tx = Transaction::new(&[&payer], msg, blockhash);
    let result = svm.send_transaction(tx);

    match result {
        Ok(_) => {
            assert_eq!(
                svm.get_account(&dest).unwrap().lamports,
                5_000_000,
                "all lamports moved to dest"
            );
            assert_eq!(
                svm.get_account(&target).map(|a| a.lamports).unwrap_or(0),
                0,
                "target account drained"
            );
        }
        Err(ref e) => {
            // Report the failure with full details so the error is diagnosable
            panic!(
                "close tx failed: {:?}\nlogs: {:?}",
                e.err,
                e.meta.logs
            );
        }
    }
}

// ── M9 litesvm tests: realloc / zero / init_if_needed ────────────────────────

/// Helper: send a single-tag instruction to the program, return Ok/Err.
fn send(
    svm: &mut LiteSVM,
    program_id: Pubkey,
    payer: &Keypair,
    tag: u8,
    metas: Vec<AccountMeta>,
) -> Result<(), ()> {
    let ix = Instruction { program_id, data: vec![tag], accounts: metas };
    let bh = svm.latest_blockhash();
    let tx = Transaction::new(&[payer], Message::new(&[ix], Some(&payer.pubkey())), bh);
    svm.send_transaction(tx).map(|_| ()).map_err(|_| ())
}

// ── realloc: grow ────────────────────────────────────────────────────────────

/// Growing a vault from 32 bytes to 80 bytes tops up lamports to the new rent-exempt minimum
/// and increases `data.len()` to exactly 80.
#[test]
fn realloc_grow_increases_data_len_and_tops_up_lamports() {
    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    svm.add_program_from_file(program_id, so_path()).expect("load .so");

    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000).unwrap();

    let vault = Pubkey::new_unique();
    // Fund vault at rent-exempt level for 32 bytes (~1_113_600 lamports) but below
    // the rent-exempt minimum for 80 bytes (~1_447_680 lamports), so the top-up fires.
    let initial_vault_lamports = 1_200_000u64;
    svm.set_account(
        vault,
        Account {
            lamports: initial_vault_lamports,
            data: vec![0u8; 32],
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    // Tag 7 = ReallocGrow (realloc = 80).
    let result = send(
        &mut svm,
        program_id,
        &payer,
        7,
        vec![
            AccountMeta::new(vault, false),          // vault: mut
            AccountMeta::new(payer.pubkey(), true),  // payer: mut signer
            AccountMeta::new_readonly(system_program_id(), false),
        ],
    );
    assert!(result.is_ok(), "realloc grow tx must succeed");

    let after = svm.get_account(&vault).expect("vault still exists");
    assert_eq!(after.data.len(), 80, "data length must be 80 after grow");
    // The payer topped up the vault to cover the rent-exempt minimum for 80 bytes.
    // Minimum for 80 bytes ≈ (128+80) * 3480 * 2 = 1_447_680 lamports.
    assert!(
        after.lamports > initial_vault_lamports,
        "payer must have topped up the vault: lamports went from {} to {} (expected increase)",
        initial_vault_lamports,
        after.lamports
    );
}

// ── realloc: shrink ───────────────────────────────────────────────────────────

/// Shrinking a vault from 80 bytes to 32 bytes PRESERVES lamports (surplus-preserving guarantee
/// certified by the Lean proof): the runtime does not drain excess lamports.
#[test]
fn realloc_shrink_preserves_lamports() {
    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    svm.add_program_from_file(program_id, so_path()).expect("load .so");

    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000).unwrap();

    let vault = Pubkey::new_unique();
    // Fund vault generously for 80 bytes (well above rent-exempt minimum).
    let initial_vault_lamports = 5_000_000u64;
    svm.set_account(
        vault,
        Account {
            lamports: initial_vault_lamports,
            data: vec![0u8; 80],
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    // Tag 8 = ReallocShrink (realloc = 32).
    let result = send(
        &mut svm,
        program_id,
        &payer,
        8,
        vec![
            AccountMeta::new(vault, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program_id(), false),
        ],
    );
    assert!(result.is_ok(), "realloc shrink tx must succeed");

    let after = svm.get_account(&vault).expect("vault still exists");
    assert_eq!(after.data.len(), 32, "data length must be 32 after shrink");
    // Surplus-preserving: lamports MUST NOT be drained.
    assert_eq!(
        after.lamports,
        initial_vault_lamports,
        "lamports must be exactly preserved after shrink (surplus-preserving guarantee)"
    );
}

// ── zero: accept / reject ─────────────────────────────────────────────────────

/// `#[account(zero)]` accepts an account whose first 8 bytes are all-zero.
#[test]
fn zero_accepts_zeroed_discriminator() {
    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    svm.add_program_from_file(program_id, so_path()).expect("load .so");

    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000).unwrap();

    let data_acct = Pubkey::new_unique();
    // 8 zero bytes — the discriminator slot is all-zero → accepted by `zero`.
    svm.set_account(
        data_acct,
        Account {
            lamports: 2_000_000,
            data: vec![0u8; 8],
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    // Tag 9 = ZeroCheck.
    let result = send(
        &mut svm,
        program_id,
        &payer,
        9,
        vec![AccountMeta::new_readonly(data_acct, false)],
    );
    assert!(result.is_ok(), "zero constraint must accept an all-zero discriminator");
}

/// `#[account(zero)]` rejects an account whose discriminator is non-zero (reinit guard).
#[test]
fn zero_rejects_nonzero_discriminator() {
    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    svm.add_program_from_file(program_id, so_path()).expect("load .so");

    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000).unwrap();

    let data_acct = Pubkey::new_unique();
    // First byte is non-zero — the discriminator is set → rejected by `zero`.
    let mut disc = vec![0u8; 8];
    disc[0] = 0xAB;
    svm.set_account(
        data_acct,
        Account {
            lamports: 2_000_000,
            data: disc,
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let result = send(
        &mut svm,
        program_id,
        &payer,
        9,
        vec![AccountMeta::new_readonly(data_acct, false)],
    );
    assert!(result.is_err(), "zero constraint must reject a non-zero discriminator");
}

// ── init_if_needed ────────────────────────────────────────────────────────────

/// The program id baked into the `verified-anchor-program` binary as `crate::ID`.
/// Must match `pub const ID` in `verified-anchor-program/src/lib.rs`.
fn typed_program_id() -> Pubkey {
    Pubkey::new_from_array([0x0Bu8; 32])
}

/// First call with a fresh (system-owned, empty) account initialises it:
/// the account becomes program-owned and has at least 8 bytes of data.
#[test]
fn init_if_needed_first_call_creates_account() {
    let mut svm = LiteSVM::new();
    let program_id = typed_program_id();
    svm.add_program_from_file(program_id, so_path()).expect("load .so");

    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000).unwrap();

    let data_acct = Keypair::new();
    // Account does not exist yet (system-owned, empty) — first call must create it.

    let ix = Instruction {
        program_id,
        data: vec![10u8], // tag 10 = InitIfNeeded
        accounts: vec![
            AccountMeta::new(data_acct.pubkey(), true),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program_id(), false),
        ],
    };
    let bh = svm.latest_blockhash();
    let tx = Transaction::new(&[&payer, &data_acct], Message::new(&[ix], Some(&payer.pubkey())), bh);
    let result = svm.send_transaction(tx);
    assert!(
        result.is_ok(),
        "init_if_needed first call must succeed: {:?}",
        result.err().map(|e| e.meta.logs)
    );

    let created = svm
        .get_account(&data_acct.pubkey())
        .expect("account must exist after first call");
    assert_eq!(
        created.owner, program_id,
        "account must be owned by the program after init"
    );
    assert!(
        created.data.len() >= 8,
        "account must have at least 8 bytes after init"
    );
    assert!(created.lamports > 0, "account must be funded after init");
}

/// Second call on an already-initialised account succeeds without re-creating it
/// (the needs-init guard short-circuits when the discriminator is non-zero).
#[test]
fn init_if_needed_second_call_accepted_not_reinit() {
    let mut svm = LiteSVM::new();
    let program_id = typed_program_id();
    svm.add_program_from_file(program_id, so_path()).expect("load .so");

    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000).unwrap();

    let data_acct = Keypair::new();

    // ── first call: initialise the account ──
    let ix1 = Instruction {
        program_id,
        data: vec![10u8],
        accounts: vec![
            AccountMeta::new(data_acct.pubkey(), true),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program_id(), false),
        ],
    };
    let bh1 = svm.latest_blockhash();
    let tx1 = Transaction::new(
        &[&payer, &data_acct],
        Message::new(&[ix1], Some(&payer.pubkey())),
        bh1,
    );
    svm.send_transaction(tx1).expect("first call must succeed");

    let after_first = svm
        .get_account(&data_acct.pubkey())
        .expect("account exists after first call");
    let lamports_after_first = after_first.lamports;

    // ── second call: account already initialised, must not re-init ──
    let ix2 = Instruction {
        program_id,
        data: vec![10u8],
        accounts: vec![
            AccountMeta::new(data_acct.pubkey(), false), // not a signer on second call
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program_id(), false),
        ],
    };
    let bh2 = svm.latest_blockhash();
    let tx2 = Transaction::new(&[&payer], Message::new(&[ix2], Some(&payer.pubkey())), bh2);
    let result2 = svm.send_transaction(tx2);
    assert!(
        result2.is_ok(),
        "init_if_needed second call must succeed (already initialised): {:?}",
        result2.err().map(|e| e.meta.logs)
    );

    let after_second = svm
        .get_account(&data_acct.pubkey())
        .expect("account still exists after second call");

    // Lamports must not change: no top-up was needed (account already exists and is funded).
    assert_eq!(
        after_second.lamports, lamports_after_first,
        "lamports must not change on second call (no re-init)"
    );
    // Owner unchanged.
    assert_eq!(
        after_second.owner, program_id,
        "owner must remain the program after second call"
    );
}
