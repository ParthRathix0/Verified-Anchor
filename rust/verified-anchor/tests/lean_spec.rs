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
{ programId := Pubkey.zero
, fields :=
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
{ programId := Pubkey.zero
, fields :=
  [ { name := \"pda\", ty := AccountType.uncheckedAccount, constraints := [Constraint.seeds [SeedSpec.literal (ByteArray.mk #[118, 97, 117, 108, 116]), SeedSpec.instrArg 0 4] BumpSpec.canonical] } ] }";
    assert_eq!(PdaSpec::lean_spec(), expected);
}
