//! M10 Task 15: on-chain proof for the new constraint-expr sublanguage. Native tests build
//! `AccountInfo`s by hand; only litesvm runs the generated code inside a real loader against
//! real Borsh-encoded account data. The M5 `inventory` incident — a change that passed every
//! native test while corrupting the SBF ELF — is why this suite is load-bearing, not a
//! formality. Loads `verified_anchor_program.so` (see `verified-anchor-program/src/lib.rs`,
//! tags 11/12/13) and asserts, per scenario: attacker input rejected, legitimate input
//! accepted, and an observable on-chain effect.
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

fn setup(program_id: Pubkey) -> (LiteSVM, Keypair) {
    let mut svm = LiteSVM::new();
    svm.add_program_from_file(program_id, so_path())
        .expect("load .so (run cargo-build-sbf first)");
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000).unwrap();
    (svm, payer)
}

fn send(
    svm: &mut LiteSVM,
    program_id: Pubkey,
    payer: &Keypair,
    data: Vec<u8>,
    metas: Vec<AccountMeta>,
) -> Result<(), ()> {
    let ix = Instruction { program_id, data, accounts: metas };
    let bh = svm.latest_blockhash();
    let tx = Transaction::new(&[payer], Message::new(&[ix], Some(&payer.pubkey())), bh);
    svm.send_transaction(tx).map(|_| ()).map_err(|_| ())
}

// ── Scenario 1: `constraint = vault.amount >= 1000` over a field AFTER a `String` ───────────
//
// `Account<'info, T>` implies `owner = crate::ID`, so the program must be loaded under the
// `crate::ID` baked into `verified-anchor-program` — `[0x0Bu8; 32]` — the same convention the
// existing `init_if_needed` litesvm suite (`runtime_lifecycle.rs`) uses for typed accounts.

fn typed_program_id() -> Pubkey {
    Pubkey::new_from_array([0x0Bu8; 32])
}

/// DataVault::DISCRIMINATOR = sha256(b"account:DataVault")[..8] — recomputed here so the test
/// builds the exact wire bytes the codegen expects, independent of the program crate.
fn data_vault_discriminator() -> [u8; 8] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"account:DataVault");
    let out = h.finalize();
    let mut d = [0u8; 8];
    d.copy_from_slice(&out[..8]);
    d
}

/// Borsh-encode a `DataVault { label: String, amount: u64 }` account: discriminator + 4-byte LE
/// length-prefixed `label` + 8-byte LE `amount`. `amount`'s offset depends on `label`'s length —
/// exactly the length-prefix walk this scenario is proving works on-chain.
fn data_vault_account(label: &str, amount: u64) -> Vec<u8> {
    let mut d = data_vault_discriminator().to_vec();
    d.extend_from_slice(&(label.len() as u32).to_le_bytes());
    d.extend_from_slice(label.as_bytes());
    d.extend_from_slice(&amount.to_le_bytes());
    d
}

/// Attacker input (amount below the constraint) is rejected; legitimate input (amount at/above
/// the constraint) is accepted; the accepted `amount` is echoed into `out` — the observable
/// effect, read by the SAME length-prefix walk the constraint check performed.
#[test]
fn constraint_after_string_field_enforced_onchain() {
    let program_id = typed_program_id();
    let (mut svm, payer) = setup(program_id);

    let out = Pubkey::new_unique();
    svm.set_account(out, Account {
        lamports: 1, data: vec![0u8; 8], owner: program_id, executable: false, rent_epoch: 0,
    }).unwrap();

    // A label whose length is NOT a multiple of 8/4 — proves the offset math is real prefix
    // arithmetic, not an accidentally-aligned constant.
    let label = "on-chain-length-prefix-proof";

    let attacker_vault = Pubkey::new_unique();
    svm.set_account(attacker_vault, Account {
        lamports: 1, data: data_vault_account(label, 500), // below the 1000 threshold
        owner: program_id, executable: false, rent_epoch: 0,
    }).unwrap();
    let legit_vault = Pubkey::new_unique();
    svm.set_account(legit_vault, Account {
        lamports: 1, data: data_vault_account(label, 5_000), // at/above the threshold
        owner: program_id, executable: false, rent_epoch: 0,
    }).unwrap();

    let metas = |vault: Pubkey| vec![
        AccountMeta::new_readonly(vault, false),
        AccountMeta::new(out, false),
    ];

    // Attacker input rejected.
    assert!(
        send(&mut svm, program_id, &payer, vec![11u8], metas(attacker_vault)).is_err(),
        "amount below 1000 (positioned after a String) must be rejected on-chain"
    );

    // Legitimate input accepted, with an observable on-chain effect.
    assert!(
        send(&mut svm, program_id, &payer, vec![11u8], metas(legit_vault)).is_ok(),
        "amount at/above 1000 (positioned after a String) must be accepted on-chain"
    );
    let out_data = svm.get_account(&out).unwrap().data;
    assert_eq!(
        u64::from_le_bytes(out_data[..8].try_into().unwrap()),
        5_000,
        "accepted amount, read by walking past the String's length prefix, must land in `out`"
    );
}

// ── Scenario 2: `#[instruction(name: String)]` seed via `name.as_bytes()` ───────────────────
//
// No `crate::ID` owner check is involved (`UncheckedAccount`), so any program id works, exactly
// like the existing `CheckPda` (tag 2) scenario in `runtime_seeds.rs`.

/// Borsh-encode a single `String` instruction argument the way a real client would: 4-byte LE
/// length prefix, then the payload.
fn borsh_string_arg(s: &str) -> Vec<u8> {
    let mut v = (s.len() as u32).to_le_bytes().to_vec();
    v.extend_from_slice(s.as_bytes());
    v
}

/// Attacker input (a key that is not the PDA derived from the decoded `name` argument) is
/// rejected; the legitimate PDA is accepted; the accepted PDA's key is echoed into `out`.
#[test]
fn named_instruction_arg_seed_enforced_onchain() {
    let program_id = Pubkey::new_unique();
    let (mut svm, payer) = setup(program_id);

    let out = Pubkey::new_unique();
    svm.set_account(out, Account {
        lamports: 1, data: vec![0u8; 32], owner: program_id, executable: false, rent_epoch: 0,
    }).unwrap();

    let (pda, _bump) = Pubkey::find_program_address(&[b"vault", b"alice"], &program_id);
    let attacker_pda = Pubkey::new_unique(); // not the PDA derived from "alice"

    let mut ix_data = vec![12u8];
    ix_data.extend_from_slice(&borsh_string_arg("alice"));

    let metas = |pda: Pubkey| vec![
        AccountMeta::new_readonly(pda, false),
        AccountMeta::new(out, false),
    ];

    // Attacker input rejected.
    assert!(
        send(&mut svm, program_id, &payer, ix_data.clone(), metas(attacker_pda)).is_err(),
        "wrong PDA under the decoded `name` argument must be rejected on-chain"
    );

    // Legitimate input accepted, with an observable on-chain effect.
    assert!(
        send(&mut svm, program_id, &payer, ix_data, metas(pda)).is_ok(),
        "the PDA derived from the on-chain-decoded `name` argument must be accepted"
    );
    assert_eq!(
        &svm.get_account(&out).unwrap().data[..32],
        pda.as_ref(),
        "accepted PDA key must be echoed into `out`"
    );
}

// ── Scenario 3: escape-hatch constraint executes in `try_accounts` on-chain ─────────────────
//
// `constraint = a.key() == crate::ID` is outside the proven sublanguage (a module-qualified
// path), so it runs through the escape hatch. The hatch executes ONLY inside `try_accounts` —
// never inside `Validate::validate` alone — so the program arm for tag 13 deliberately calls
// `try_accounts` (see `verified-anchor-program/src/lib.rs`), and this test exists specifically
// to prove that path runs on a real loader, not just in the native `try_accounts` unit test in
// `behavior.rs`.

/// Attacker input (a key that is not `crate::ID`) is rejected; the legitimate key (exactly
/// `crate::ID`) is accepted; `out` is flipped as the observable effect.
#[test]
fn escape_hatch_constraint_runs_in_try_accounts_onchain() {
    let program_id = Pubkey::new_unique();
    let (mut svm, payer) = setup(program_id);

    let out = Pubkey::new_unique();
    svm.set_account(out, Account {
        lamports: 1, data: vec![0u8; 8], owner: program_id, executable: false, rent_epoch: 0,
    }).unwrap();

    // `crate::ID` baked into `verified-anchor-program`.
    let matching_key = Pubkey::new_from_array([0x0Bu8; 32]);
    let other_key = Pubkey::new_unique();

    let metas = |a: Pubkey| vec![
        AccountMeta::new_readonly(a, false),
        AccountMeta::new(out, false),
    ];

    // Attacker input (key != crate::ID) rejected.
    assert!(
        send(&mut svm, program_id, &payer, vec![13u8], metas(other_key)).is_err(),
        "escape-hatch constraint `a.key() == crate::ID` must reject a non-matching key on-chain"
    );

    // Legitimate input (key == crate::ID) accepted, with an observable on-chain effect.
    assert!(
        send(&mut svm, program_id, &payer, vec![13u8], metas(matching_key)).is_ok(),
        "escape-hatch constraint must accept the matching key via try_accounts on-chain"
    );
    assert_eq!(
        svm.get_account(&out).unwrap().data[0], 1,
        "the hatch must actually have RUN — an unreachable hatch would never flip this byte"
    );
}
