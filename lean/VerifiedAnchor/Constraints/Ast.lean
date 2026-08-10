import VerifiedAnchor.Solana.Pubkey
import VerifiedAnchor.Solana.Layout
import VerifiedAnchor.Solana.Discriminator

namespace VerifiedAnchor

/-- A single seed in a PDA derivation. -/
inductive SeedSpec where
  | literal (bytes : ByteArray)        -- e.g. b"vault"
  | fieldKey (field : String)          -- another account's key bytes
  | instrArg (off : Nat) (len : Nat)   -- a concrete slice of the instruction data
  deriving Inhabited

inductive BumpSpec where
  | declared (b : UInt8)
  | canonical
  /-- Opt-in, non-canonical "stored" bump: the bump byte is read from the instruction data
      at byte offset `argOff`. The PDA is derived with THAT specific bump via
      `createProgramAddress` — there is NO canonical `findProgramAddress` requirement. This is
      the deliberately less-safe explicit opt-in; canonical stays the safe default. -/
  | stored (argOff : Nat)
  deriving Inhabited, DecidableEq

/-- The Anchor constraint subset in scope for v1. -/
inductive Constraint where
  | signer
  | mut
  | owner          (expected : Pubkey)
  | hasOne         (field : String)
  /-- `program` is the `seeds::program = <expr>` override: `none` ⇒ derive the PDA against the
      struct's own `s.programId` (back-compat); `some p` ⇒ derive against the FOREIGN id `p`. -/
  | seeds          (seeds : List SeedSpec) (bump : BumpSpec) (program : Option Pubkey)
  | init           (payer : String) (space : Nat) (owner : Pubkey)
  | close          (dest : String)
  | discriminator  (expected : ByteArray)   -- 8 bytes
  | executable                              -- account is executable (Program<P> base check)
  | address        (expected : Pubkey)      -- account key equals `expected` (Program<P> id)
  /-- `rent_exempt = enforce`: the account holds at least the rent-exempt minimum lamports for
      its data size. The minimum is the OPAQUE `rentExemptMinimum a.data.size` (an uninterpreted
      wall like `sha256`, cross-checked empirically by litesvm). `rent_exempt = skip` emits NO
      constraint (the documented SAFE-BY-DEFAULT opt-out). -/
  | rentExempt
  /-- `realloc = newLen` (+ `realloc::payer`, `realloc::zero`): resize the account's data to
      `newLen` total bytes, top up lamports from `payer` to stay rent-exempt (never debits the
      account), and (when `zero`) zero-fill the grown region. A lifecycle marker — not a
      validation constraint (`genConstraint` returns false); modelled by `applyRealloc`. -/
  | realloc        (payer : String) (newLen : Nat) (zero : Bool)
  /-- `zero`: the account's 8-byte discriminator is currently all-zero (allocated, never
      initialized) — the reinit guard. A VALIDATION constraint (crypto-free; reduces under
      `decide`). -/
  | zero
  /-- `init_if_needed`: init the account when uninitialized, else accept the existing valid
      account. A lifecycle marker; modelled by `applyInitIfNeeded`. -/
  | initIfNeeded   (payer : String) (space : Nat) (owner : Pubkey)
  deriving Inhabited

/-- Whether a constraint is the `mut` (writable) marker. A constructor test rather than full
    `DecidableEq Constraint` — `Constraint` carries `ByteArray` payloads that lack
    `DecidableEq`, so we test the single constructor the distinct-mut-key check cares about. -/
def Constraint.isMut : Constraint → Bool
  | .mut => true
  | _    => false

/-- Account wrapper types; each implies certain base constraints. -/
inductive AccountType where
  | account          (typeName : String) (layout : FieldLayout) (programId : Pubkey)
  | signer
  | program          (id : Pubkey)
  | systemAccount
  | uncheckedAccount
  deriving Inhabited

/-- Base constraints implied by the wrapper type, before explicit annotations.

    `systemAccount` and `program` model the runtime base checks the macro's `wrapper_implied`
    emits in `validate`: a `SystemAccount<'info>` is owned by the System Program, and a
    `Program<'info, P>` is executable with key `P::ID`. The concrete pubkey is a placeholder
    (`Pubkey.zero`, the System-Program placeholder) — `genValidate_sound` is schematic over it,
    exactly like the explicit `owner = EXPR` placeholder. -/
def AccountType.impliedConstraints : AccountType → List Constraint
  | .account tn _ pid => [Constraint.owner pid, Constraint.discriminator (accountDiscriminator tn)]
  | .signer           => [Constraint.signer]
  | .program id       => [Constraint.executable, Constraint.address id]
  | .systemAccount    => [Constraint.owner Pubkey.zero]
  | .uncheckedAccount => []

/-- Look up the layout offset of a `Pubkey` field within an account type. -/
def AccountType.layoutOffsetOf : AccountType → String → Option Nat
  | .account _ layout _, name => layout.offsetOf name
  | _, _ => none

structure AccountField where
  name        : String
  ty          : AccountType
  constraints : List Constraint
  /-- Per-field opt-out for the struct-level distinct-mutable-keys check (M8.4): the names of
      fields THIS field is explicitly permitted to alias. A `mut`/`mut` pair `(i, fi), (j, fj)`
      is EXEMPT iff `fj.name ∈ fi.allowDuplicate ∨ fi.name ∈ fj.allowDuplicate`. The default
      `[]` keeps every existing `{ name, ty, constraints }` literal compiling unchanged. -/
  allowDuplicate : List String := []
  deriving Inhabited

structure AccountsStruct where
  programId : Pubkey
  fields    : List AccountField
  deriving Inhabited

/-- Find a declared field by name. -/
def AccountsStruct.fieldNamed (s : AccountsStruct) (name : String) : Option AccountField :=
  s.fields.find? (·.name == name)

end VerifiedAnchor
