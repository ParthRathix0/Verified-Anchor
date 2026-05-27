import VerifiedAnchor.Decision.Agreement

namespace VerifiedAnchor.Examples
open VerifiedAnchor

/-! # Worked example: Anchor's `Withdraw` accounts struct

The proposal's motivating struct:
```rust
#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut, has_one = authority)]
    pub vault: Account<'info, Vault>,
    pub authority: Signer<'info>,
}
```

A real `Account<'info, Vault>` carries an *implied* `discriminator (sha256 "account:Vault")`
constraint. `sha256` is `opaque` by design, so that one check does not reduce under
`decide`/`#eval`. We therefore split the demonstration into two honest halves:

* `withdraw` — a fully concrete struct whose every constraint is crypto-free, so the
  executable `validatesBool` reduces end-to-end (accept the good context, reject the
  tampered one), and we prove the declarative `validates` contract directly.
* `withdrawTyped` — the faithful typed struct (`Account<Vault>` + `has_one`), on which we
  exercise the relational `has_one` check per-constraint via `checkConstraint` (which does
  not touch the opaque discriminator). This is the proposal's headline bug class. -/

/-- Program id for the example. -/
def progId : Pubkey := Pubkey.ofBytes (List.replicate 32 7)

/-- A concrete 8-byte discriminator placeholder. -/
def vaultDisc : ByteArray := (⟨#[1, 2, 3, 4, 5, 6, 7, 8]⟩ : ByteArray)

/-- Vault layout: `authority : Pubkey` stored right after the 8-byte discriminator. -/
def vaultLayout : FieldLayout := [("authority", 8)]

/-- The authority key used in the good contexts. -/
def authKey : Pubkey := Pubkey.ofBytes (List.replicate 32 9)

/-- Vault account data: 8-byte discriminator ++ the stored authority pubkey. -/
def vaultData (storedAuth : Pubkey) : ByteArray :=
  vaultDisc ++ ByteArray.mk storedAuth.toArray

/-- A vault account whose stored `authority` field is `storedAuth`, owned by `owner`. -/
def mkVault (storedAuth owner : Pubkey) : AccountInfo :=
  { key := Pubkey.ofBytes (List.replicate 32 1), lamports := 100,
    data := vaultData storedAuth, owner := owner, rentEpoch := 0,
    isSigner := false, isWritable := true, executable := false }

/-- The authority account: a signer whose key is `authKey`. -/
def authorityAccount : AccountInfo :=
  { key := authKey, lamports := 1, data := ByteArray.empty, owner := Pubkey.zero,
    rentEpoch := 0, isSigner := true, isWritable := false, executable := false }

def authorityField : AccountField :=
  { name := "authority", ty := AccountType.signer, constraints := [] }

/-! ## Part 1 — fully computable: `validatesBool` accepts good, rejects tampered -/

/-- Concrete vault field: unchecked account with crypto-free constraints
    `mut`, `owner = progId`, and an explicit concrete `discriminator`. -/
def vaultField : AccountField :=
  { name := "vault"
  , ty := AccountType.uncheckedAccount
  , constraints := [Constraint.mut, Constraint.owner progId, Constraint.discriminator vaultDisc] }

def withdraw : AccountsStruct where
  programId := progId
  fields := [vaultField, authorityField]

/-- GOOD context: writable, owned by the program, correct discriminator, real signer. -/
def goodCtx : Ctx := [mkVault authKey progId, authorityAccount]

/-- TAMPERED context: the vault is owned by some other program (owner check must fail). -/
def tamperedCtx : Ctx := [mkVault authKey (Pubkey.ofBytes (List.replicate 32 3)), authorityAccount]

-- For human eyes:
#eval validatesBool withdraw goodCtx       -- expect true
#eval validatesBool withdraw tamperedCtx   -- expect false

-- Build-time assertions: the checker accepts the good context and rejects the tampered one.
#guard validatesBool withdraw goodCtx = true
#guard validatesBool withdraw tamperedCtx = false

-- The discriminator machinery bites on concrete data: a wrong discriminator fails.
#guard checkConstraint withdraw goodCtx 0 vaultField (Constraint.discriminator vaultDisc) = true
#guard checkConstraint withdraw goodCtx 0 vaultField
         (Constraint.discriminator (⟨#[9, 9, 9, 9, 9, 9, 9, 9]⟩ : ByteArray)) = false

/-- The good context provably satisfies the declarative contract. -/
theorem good_validates : validates withdraw goodCtx := by
  apply validatesBool_sound
  decide

/-- The tampered context provably does NOT satisfy the contract. -/
theorem tampered_not_validates : ¬ validates withdraw tamperedCtx := by
  rw [validates_iff_validatesBool]
  decide

/-! ## Part 2 — faithful typed struct: the relational `has_one` check

This is the proposal's headline bug class. `has_one` is only valid on a typed
`Account<T>`, which is why `vaultFieldTyped` uses `AccountType.account`. We probe the
relational constraint directly with `checkConstraint`, which evaluates only that
constraint and so never touches the opaque discriminator. -/

def vaultFieldTyped : AccountField :=
  { name := "vault"
  , ty := AccountType.account "Vault" vaultLayout progId
  , constraints := [Constraint.mut, Constraint.hasOne "authority"] }

def withdrawTyped : AccountsStruct where
  programId := progId
  fields := [vaultFieldTyped, authorityField]

/-- GOOD: the vault's stored authority matches the signer. -/
def goodCtxT : Ctx := [mkVault authKey progId, authorityAccount]

/-- TAMPERED: the vault's stored authority does NOT match the signer
    (the classic missing/forged `has_one` exploit). -/
def tamperedCtxT : Ctx := [mkVault (Pubkey.ofBytes (List.replicate 32 2)) progId, authorityAccount]

-- The relational `has_one` accepts the matching context and rejects the forged one:
#guard checkConstraint withdrawTyped goodCtxT 0 vaultFieldTyped (Constraint.hasOne "authority") = true
#guard checkConstraint withdrawTyped tamperedCtxT 0 vaultFieldTyped (Constraint.hasOne "authority") = false

end VerifiedAnchor.Examples
