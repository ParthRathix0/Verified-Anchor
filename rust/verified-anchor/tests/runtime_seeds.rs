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

/// Find a GENUINELY NON-CANONICAL off-curve bump: strictly below the canonical bump.
/// `find_program_address` returns the HIGHEST off-curve bump (the canonical one), so we
/// search ascending from 0 up to (but not including) `canon_bump` for the first b where
/// `create_program_address` succeeds.  The derived address differs from the canonical PDA.
fn non_canonical_stored_bump(program_id: &Pubkey) -> (u8, Pubkey, Pubkey) {
    let (canon_key, canon_bump) = Pubkey::find_program_address(&[b"vault"], program_id);
    for b in 0u8..canon_bump {
        if let Ok(pk) = Pubkey::create_program_address(&[b"vault", &[b]], program_id) {
            return (b, pk, canon_key);
        }
    }
    panic!("no non-canonical off-curve bump found below canonical bump {canon_bump} for b\"vault\"");
}

/// The opt-in `bump = arg(0)` (stored, non-canonical) check accepts the address derived with
/// a GENUINELY NON-CANONICAL bump (strictly below canonical) — on-chain, no canonical
/// requirement.  The assert_ne! proves the accepted PDA differs from the canonical one.
#[test]
fn seeds_stored_bump_good_accepted_onchain() {
    let (mut svm, program_id, payer) = setup();
    let (stored_bump, pda, canon_key) = non_canonical_stored_bump(&program_id);
    assert_ne!(pda, canon_key, "non-canonical PDA must differ from canonical PDA");

    let data = vec![3u8, stored_bump]; // tag 3 + stored bump byte at offset 0

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
        "PDA derived with the stored bump should validate on-chain"
    );
}

/// A key that is not the stored-bump-derived PDA is rejected on-chain.
#[test]
fn seeds_stored_bump_wrong_pda_rejected_onchain() {
    let (mut svm, program_id, payer) = setup();
    let (stored_bump, _pda, _canon) = non_canonical_stored_bump(&program_id);
    let wrong = Pubkey::new_unique();

    let data = vec![3u8, stored_bump];

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
        "wrong stored-bump PDA must be rejected on-chain"
    );
}

/// The FOREIGN program id the on-chain `CheckForeignPda` (tag 4) derives its PDA against.
/// Mirrors `FOREIGN_PROGRAM` in the program crate (`[9u8; 32]`).
const FOREIGN_PROGRAM: Pubkey = Pubkey::new_from_array([9u8; 32]);

/// `seeds::program = FOREIGN_PROGRAM`: the on-chain PDA check accepts the address derived
/// against the FOREIGN program id (NOT the running program's own id).
#[test]
fn seeds_program_foreign_pda_accepted_onchain() {
    let (mut svm, program_id, payer) = setup();
    // Derived against the FOREIGN id, while the tx runs under `program_id`.
    let (foreign_pda, _bump) = Pubkey::find_program_address(&[b"vault"], &FOREIGN_PROGRAM);

    let ix = Instruction {
        program_id,
        data: vec![4u8], // tag 4 = CheckForeignPda
        accounts: vec![AccountMeta::new_readonly(foreign_pda, false)],
    };
    let blockhash = svm.latest_blockhash();
    let msg = Message::new(&[ix], Some(&payer.pubkey()));
    let tx = Transaction::new(&[&payer], msg, blockhash);
    assert!(
        svm.send_transaction(tx).is_ok(),
        "foreign-program-derived PDA should validate on-chain"
    );
}

/// The PDA derived against the running program's OWN id is the WRONG one under
/// `seeds::program = FOREIGN_PROGRAM`, so it is rejected on-chain (proves the override bites).
#[test]
fn seeds_program_own_program_pda_rejected_onchain() {
    let (mut svm, program_id, payer) = setup();
    let (own_pda, _bump) = Pubkey::find_program_address(&[b"vault"], &program_id);
    let (foreign_pda, _fb) = Pubkey::find_program_address(&[b"vault"], &FOREIGN_PROGRAM);
    assert_ne!(own_pda, foreign_pda, "own-program PDA must differ from the foreign one");

    let ix = Instruction {
        program_id,
        data: vec![4u8],
        accounts: vec![AccountMeta::new_readonly(own_pda, false)],
    };
    let blockhash = svm.latest_blockhash();
    let msg = Message::new(&[ix], Some(&payer.pubkey()));
    let tx = Transaction::new(&[&payer], msg, blockhash);
    assert!(
        svm.send_transaction(tx).is_err(),
        "own-program PDA must be rejected when seeds::program targets a foreign id"
    );
}
