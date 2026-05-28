use litesvm::LiteSVM;
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

/// The generated `seeds = [b"vault", arg(0,4)]` PDA check accepts the canonically-derived
/// address on-chain.
#[test]
fn seeds_good_pda_accepted_onchain() {
    let (mut svm, program_id, payer) = setup();
    let arg = [1u8, 2, 3, 4];
    let (pda, _bump) = Pubkey::find_program_address(&[b"vault", &arg], &program_id);

    let mut data = vec![2u8]; // instruction tag
    data.extend_from_slice(&arg); // 4-byte seed arg

    let ix = Instruction {
        program_id,
        data,
        accounts: vec![AccountMeta::new_readonly(pda, false)],
    };
    let blockhash = svm.latest_blockhash();
    let msg = Message::new(&[ix], Some(&payer.pubkey()));
    let tx = Transaction::new(&[&payer], msg, blockhash);
    assert!(
        svm.send_transaction(tx).is_ok(),
        "correct PDA should validate on-chain"
    );
}

/// A wrong address (not the derived PDA) is rejected on-chain.
#[test]
fn seeds_wrong_pda_rejected_onchain() {
    let (mut svm, program_id, payer) = setup();
    let arg = [1u8, 2, 3, 4];
    let wrong = Pubkey::new_unique(); // not the derived PDA

    let mut data = vec![2u8];
    data.extend_from_slice(&arg);

    let ix = Instruction {
        program_id,
        data,
        accounts: vec![AccountMeta::new_readonly(wrong, false)],
    };
    let blockhash = svm.latest_blockhash();
    let msg = Message::new(&[ix], Some(&payer.pubkey()));
    let tx = Transaction::new(&[&payer], msg, blockhash);
    assert!(
        svm.send_transaction(tx).is_err(),
        "wrong PDA must be rejected on-chain"
    );
}
