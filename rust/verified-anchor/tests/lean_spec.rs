use sha2::{Digest, Sha256};
use verified_anchor::VerifiedAccounts;
use verified_anchor::{Signer, UncheckedAccount};

// Required so that Account<'info, T> (which implies owner = crate::ID) can be used in
// init_if_needed tests below.
solana_program::declare_id!("VASpec1111111111111111111111111111111111111");

#[derive(borsh::BorshSerialize, borsh::BorshDeserialize, verified_anchor_macros::AccountData)]
struct VaultAccount {
    value: u64,
}

#[derive(VerifiedAccounts)]
struct Transfer<'info> {
    #[account(mut)]
    vault: UncheckedAccount<'info>,
    authority: Signer<'info>,
}

#[test]
fn lean_spec_matches() {
    let expected = "\
{ programId := Pubkey.zero, fields :=
  [ { name := \"vault\", ty := AccountType.uncheckedAccount, constraints := [Constraint.mut] }
  , { name := \"authority\", ty := AccountType.signer, constraints := [] } ] }";
    assert_eq!(Transfer::lean_spec(), expected);
}

#[derive(VerifiedAccounts)]
struct PdaSpec<'info> {
    #[account(seeds = [b"vault", arg(0, 4)], bump)]
    pda: UncheckedAccount<'info>,
}

#[test]
fn lean_spec_seeds() {
    let expected = "\
{ programId := Pubkey.zero, fields :=
  [ { name := \"pda\", ty := AccountType.uncheckedAccount, constraints := [Constraint.seeds [SeedSpec.literal (ByteArray.mk #[118, 97, 117, 108, 116]), SeedSpec.instrArg 0 4] BumpSpec.canonical none] } ] }";
    assert_eq!(PdaSpec::lean_spec(), expected);
}

#[derive(VerifiedAccounts)]
struct PdaStoredBumpSpec<'info> {
    #[account(seeds = [b"vault"], bump = arg(0))]
    pda: UncheckedAccount<'info>,
}

/// The opt-in `bump = arg(0)` emits `BumpSpec.stored 0` — the exact constructor proven sound
/// in Lean (`genConstraint_seeds_iff`, `.stored` case).
#[test]
fn lean_spec_seeds_stored_bump() {
    let expected = "\
{ programId := Pubkey.zero, fields :=
  [ { name := \"pda\", ty := AccountType.uncheckedAccount, constraints := [Constraint.seeds [SeedSpec.literal (ByteArray.mk #[118, 97, 117, 108, 116])] (BumpSpec.stored 0) none] } ] }";
    assert_eq!(PdaStoredBumpSpec::lean_spec(), expected);
}

const FOREIGN_PROG: verified_anchor::solana_program::pubkey::Pubkey =
    verified_anchor::solana_program::pubkey::Pubkey::new_from_array([7u8; 32]);

#[derive(VerifiedAccounts)]
struct PdaForeignProgramSpec<'info> {
    #[account(seeds = [b"vault"], seeds::program = FOREIGN_PROG, bump)]
    pda: UncheckedAccount<'info>,
}

/// `seeds::program = <expr>` emits the schematic `(some Pubkey.zero)` third field — the same
/// ∀-over-pubkey placeholder the soundness theorem covers (à la `owner`/`address`).
#[test]
fn lean_spec_seeds_program() {
    let expected = "\
{ programId := Pubkey.zero, fields :=
  [ { name := \"pda\", ty := AccountType.uncheckedAccount, constraints := [Constraint.seeds [SeedSpec.literal (ByteArray.mk #[118, 97, 117, 108, 116])] BumpSpec.canonical (some Pubkey.zero)] } ] }";
    assert_eq!(PdaForeignProgramSpec::lean_spec(), expected);
}

#[derive(VerifiedAccounts)]
struct InitClose<'info> {
    #[account(init, payer = payer, space = 0)]
    new: UncheckedAccount<'info>,
    #[account(mut)]
    payer: UncheckedAccount<'info>,
    #[account(close = payer)]
    old: UncheckedAccount<'info>,
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
struct DiscSpec<'info> {
    #[account(discriminator = "Vault")]
    vault: UncheckedAccount<'info>,
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

// ── M9 lifecycle constraint emission tests ────────────────────────────────────────────────

#[derive(VerifiedAccounts)]
struct ZeroSpec<'info> {
    #[account(zero)]
    vault: UncheckedAccount<'info>,
}

#[test]
fn lean_spec_emits_zero_constraint() {
    let s = ZeroSpec::lean_spec();
    assert!(s.contains("Constraint.zero"), "Constraint.zero missing: {s}");
}

#[derive(VerifiedAccounts)]
struct ReallocSpec<'info> {
    #[account(mut, realloc = 64, realloc::payer = payer, realloc::zero = true)]
    data: UncheckedAccount<'info>,
    #[account(mut)]
    payer: UncheckedAccount<'info>,
}

#[test]
fn lean_spec_emits_realloc_constraint() {
    let s = ReallocSpec::lean_spec();
    assert!(s.contains("Constraint.realloc \"payer\" 64 true"), "Constraint.realloc missing: {s}");
}

#[derive(VerifiedAccounts)]
struct InitIfNeededSpec<'info> {
    #[account(init_if_needed, payer = payer, space = 64)]
    data: verified_anchor::Account<'info, VaultAccount>,
    #[account(mut)]
    payer: UncheckedAccount<'info>,
}

#[test]
fn lean_spec_emits_init_if_needed_constraint() {
    let s = InitIfNeededSpec::lean_spec();
    assert!(s.contains("Constraint.initIfNeeded \"payer\" 64 Pubkey.zero"), "Constraint.initIfNeeded missing: {s}");
}

// ── M10 Task 7: the layout is spliced at runtime ──────────────────────────────────────────
//
// `lean_spec()` is no longer a baked constant: it is a `format!` whose holes are filled from
// `<T as AccountData>::LAYOUT_LEAN`. `AccountsStruct` literals are brace-heavy, so this exact-
// equality assertion is the guard that every LITERAL brace stayed escaped (`{{`/`}}`) and only
// the intended holes became holes. An escaping slip shows up here as mangled record syntax.

#[derive(borsh::BorshSerialize, borsh::BorshDeserialize, verified_anchor_macros::AccountData)]
struct OffsetVault {
    bump: u8,
    authority: solana_program::pubkey::Pubkey,
}

/// A SECOND account type with a DIFFERENT layout, so the tripwire below can catch a
/// cross-field splice. With only one `Account` field, every hole belongs to the same type and
/// a mis-binding of hole to argument is invisible.
#[derive(borsh::BorshSerialize, borsh::BorshDeserialize, verified_anchor_macros::AccountData)]
struct LedgerEntry {
    owner: solana_program::pubkey::Pubkey,
    total: u64,
}

#[derive(VerifiedAccounts)]
struct TypedHasOne<'info> {
    #[account(has_one = authority)]
    vault: verified_anchor::Account<'info, OffsetVault>,
    ledger: verified_anchor::Account<'info, LedgerEntry>,
    authority: UncheckedAccount<'info>,
}

/// TRIPWIRE. Keep this EXACT-EQUALITY, and keep TWO `Account` fields of DIFFERENT types.
///
/// It guards two independent things:
///   1. brace escaping — an `AccountsStruct` literal is brace-heavy, so a slip in the
///      escape-then-substitute order shows up here as mangled record syntax;
///   2. hole↔argument binding — each type name and each spliced `LAYOUT_LEAN` must land under
///      ITS OWN field. `OffsetVault`/`LedgerEntry` have different layouts on purpose, so a
///      cross-field splice (one type's layout under another type's name) changes this string.
///      A one-`Account` version of this test could not catch (2) at all.
#[test]
fn lean_spec_splices_the_real_layout() {
    // owner/discriminator are NOT listed here: they are the wrapper-IMPLIED constraints Lean
    // derives from `AccountType.account` itself (`impliedConstraints`), not spec entries.
    let expected = "\
{ programId := Pubkey.zero, fields :=
  [ { name := \"vault\", ty := AccountType.account \"OffsetVault\" (Ty.struct [(\"bump\", Ty.u8), (\"authority\", Ty.pubkey)]) Pubkey.zero, constraints := [Constraint.hasOne \"authority\"] }
  , { name := \"ledger\", ty := AccountType.account \"LedgerEntry\" (Ty.struct [(\"owner\", Ty.pubkey), (\"total\", Ty.u64)]) Pubkey.zero, constraints := [] }
  , { name := \"authority\", ty := AccountType.uncheckedAccount, constraints := [] } ] }";
    assert_eq!(TypedHasOne::lean_spec(), expected);
}

// ── M10 Task 9: `#[instruction(...)]` args reach the Lean literal ─────────────────────────

// NOTE the attribute ORDER: `#[derive(..)]` first, `#[instruction(..)]` second. `instruction`
// is a DERIVE HELPER (exactly as in Anchor, whose derive is
// `#[proc_macro_derive(Accounts, attributes(account, instruction))]`), and rustc rejects a
// helper written before the derive that introduces it (`legacy_derive_helpers`, deny-by-default).
// Canonical Anchor source is therefore already in this order.
#[derive(VerifiedAccounts)]
#[instruction(name: String)]
struct SeedFromArg<'info> {
    #[account(seeds = [b"vault", name.as_bytes()], bump)]
    pda: UncheckedAccount<'info>,
}

#[test]
fn lean_spec_emits_instr_args_and_arg_field_seeds() {
    let s = SeedFromArg::lean_spec();
    assert!(s.contains("instrArgs := [(\"name\", Ty.string)]"), "spec was: {s}");
    assert!(s.contains("SeedSpec.argField \"name\""), "spec was: {s}");
}