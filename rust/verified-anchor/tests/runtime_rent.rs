//! M8.5 litesvm on-chain tests: `rent_exempt = enforce` / `rent_exempt = skip`.
//!
//! TDD order: first wrote the `enforce_rejects_under_funded` assertion (watched it fail when
//! the arm was missing from the program), then wired tag 5 / tag 6, then verified green.
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

fn setup() -> (LiteSVM, Pubkey, Keypair) {
    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    svm.add_program_from_file(program_id, so_path())
        .expect("load .so (run cargo-build-sbf first)");
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000).unwrap();
    (svm, program_id, payer)
}

fn send_tag(svm: &mut LiteSVM, payer: &Keypair, program_id: Pubkey, tag: u8, vault: Pubkey) -> bool {
    let ix = Instruction {
        program_id,
        data: vec![tag],
        accounts: vec![AccountMeta::new_readonly(vault, false)],
    };
    let bh = svm.latest_blockhash();
    let tx = Transaction::new(&[payer], Message::new(&[ix], Some(&payer.pubkey())), bh);
    svm.send_transaction(tx).is_ok()
}

/// `rent_exempt = enforce` rejects an account with lamports = 0 (clearly under the minimum).
#[test]
fn enforce_rejects_under_funded() {
    let (mut svm, program_id, payer) = setup();
    let vault = Pubkey::new_unique();
    // 0 lamports, empty data: cannot be rent-exempt.
    svm.set_account(vault, Account {
        lamports: 0,
        data: vec![],
        owner: program_id,
        executable: false,
        rent_epoch: 0,
    }).unwrap();
    assert!(
        !send_tag(&mut svm, &payer, program_id, 5, vault),
        "rent_exempt = enforce must reject an under-funded account on-chain"
    );
}

/// `rent_exempt = enforce` accepts an account with enough lamports to be rent-exempt.
/// litesvm uses Solana's real Rent sysvar: ~890_880 lamports for 0-byte data.
#[test]
fn enforce_accepts_rent_exempt_account() {
    let (mut svm, program_id, payer) = setup();
    let vault = Pubkey::new_unique();
    // 2_000_000 lamports, empty data: well above any rent-exemption threshold.
    svm.set_account(vault, Account {
        lamports: 2_000_000,
        data: vec![],
        owner: program_id,
        executable: false,
        rent_epoch: 0,
    }).unwrap();
    assert!(
        send_tag(&mut svm, &payer, program_id, 5, vault),
        "rent_exempt = enforce must accept a sufficiently-funded account on-chain"
    );
}

/// `rent_exempt = skip` accepts an under-funded account (the opt-out suppresses all checks).
#[test]
fn skip_allows_under_funded() {
    let (mut svm, program_id, payer) = setup();
    let vault = Pubkey::new_unique();
    // Same 0-lamport account that enforce would reject — skip must allow it.
    svm.set_account(vault, Account {
        lamports: 0,
        data: vec![],
        owner: program_id,
        executable: false,
        rent_epoch: 0,
    }).unwrap();
    assert!(
        send_tag(&mut svm, &payer, program_id, 6, vault),
        "rent_exempt = skip must accept any account regardless of lamports"
    );
}
