use sha2::{Digest, Sha256};
use verified_anchor::VerifiedAccounts;

#[derive(VerifiedAccounts)]
struct Transfer {
    #[account(mut)]
    vault: u8,
    #[account(signer)]
    authority: u8,
}

#[test]
fn lean_spec_matches() {
    let expected = "\
{ programId := Pubkey.zero, fields :=
  [ { name := \"vault\", ty := AccountType.uncheckedAccount, constraints := [Constraint.mut] }
  , { name := \"authority\", ty := AccountType.uncheckedAccount, constraints := [Constraint.signer] } ] }";
    assert_eq!(Transfer::lean_spec(), expected);
}

#[derive(VerifiedAccounts)]
struct PdaSpec {
    #[account(seeds = [b"vault", arg(0, 4)], bump)]
    pda: u8,
}

#[test]
fn lean_spec_seeds() {
    let expected = "\
{ programId := Pubkey.zero, fields :=
  [ { name := \"pda\", ty := AccountType.uncheckedAccount, constraints := [Constraint.seeds [SeedSpec.literal (ByteArray.mk #[118, 97, 117, 108, 116]), SeedSpec.instrArg 0 4] BumpSpec.canonical] } ] }";
    assert_eq!(PdaSpec::lean_spec(), expected);
}

#[derive(VerifiedAccounts)]
struct InitClose {
    #[account(init, payer = payer, space = 0)]
    new: u8,
    #[account(mut)]
    payer: u8,
    #[account(close = payer)]
    old: u8,
}

#[test]
fn lean_spec_emits_lifecycle_constraints() {
    let s = InitClose::lean_spec();
    assert!(s.contains("Constraint.init \"payer\" 0 Pubkey.zero"), "init missing: {s}");
    assert!(s.contains("Constraint.close \"payer\""), "close missing: {s}");
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
struct DiscSpec {
    #[account(discriminator = "Vault")]
    vault: u8,
}

#[test]
fn lean_spec_discriminator_bytes_match_anchor() {
    let d = disc("Vault");
    let expected_constraint = format!(
        "Constraint.discriminator (ByteArray.mk #[{}, {}, {}, {}, {}, {}, {}, {}])",
        d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
    );
    let s = DiscSpec::lean_spec();
    assert!(s.contains(&expected_constraint), "spec missing real-Anchor discriminator bytes:\n{s}");
}
