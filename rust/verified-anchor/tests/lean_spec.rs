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
