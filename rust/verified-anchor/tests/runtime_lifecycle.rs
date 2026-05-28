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
