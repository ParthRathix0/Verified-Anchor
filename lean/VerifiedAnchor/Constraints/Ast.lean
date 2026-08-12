import VerifiedAnchor.Solana.Pubkey
import VerifiedAnchor.Solana.Discriminator
import VerifiedAnchor.Solana.Borsh.Locate

namespace VerifiedAnchor

/-- A single seed in a PDA derivation. -/
inductive SeedSpec where
  | literal (bytes : ByteArray)        -- e.g. b"vault"
  | fieldKey (field : String)          -- another account's key bytes
  | instrArg (off : Nat) (len : Nat)   -- DEPRECATED: raw slice; kept for back-compat
  /-- A named `#[instruction(...)]` argument, resolved by Borsh-locating it in the
      instruction data. This is what real Anchor source writes (`name.as_bytes()`). -/
  | argField (name : String)
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

/-! ### The `constraint = <expr>` relational sublanguage.

    The DATATYPES live here, in `Ast.lean`, rather than in `Constraints/Expr.lean` where their
    evaluator lives. `Constraint.expr` (below) needs `Expr`, and `evalExpr` needs `Ctx` — which
    is defined in `Context.lean`, which imports THIS file. Putting the whole sublanguage in
    `Expr.lean` would therefore close the cycle `Ast → Expr → Context → Ast`. Splitting on the
    syntax/semantics seam breaks it: syntax has no `Ctx` dependency, semantics has no
    `Constraint` dependency. (`AccountsStruct.argBytes` was moved to `Context.lean` in M9 for
    exactly this reason, in the opposite direction.) -/

/-- An operand of the restricted relational sublanguage. Account operands carry the FIELD INDEX
    within the `AccountsStruct` (matching `Ctx.atField`), so an operand is meaningful independent
    of which field's `#[account(...)]` the constraint was written on. -/
inductive Operand where
  | lit        (v : Value)
  | field      (accIdx : Nat) (path : List String)   -- account data, via `locate`
  | key        (accIdx : Nat)
  | owner      (accIdx : Nat)
  | lamports   (accIdx : Nat)
  | dataLen    (accIdx : Nat)
  | isSigner   (accIdx : Nat)
  | isWritable (accIdx : Nat)
  | executable (accIdx : Nat)
  | instrArg   (name : String)
  deriving Inhabited, DecidableEq

/-- Comparison operators. `eq`/`ne` work on any two `Value`s; the four ordering operators are
    restricted to like-typed numeric pairs by `evalCmp` (see `Constraints/Expr.lean`). -/
inductive Cmp where
  | eq | ne | lt | le | gt | ge
  deriving Inhabited, DecidableEq

/-- A boolean expression over operands. Deliberately relational only — there is no arithmetic,
    so an expression can never overflow or panic, and every subterm either denotes a value or
    fails closed. -/
inductive Expr where
  | cmp    (op : Cmp) (l r : Operand)
  | and    (l r : Expr)
  | or     (l r : Expr)
  | not    (e : Expr)
  | truthy (o : Operand)
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
  /-- `constraint = <expr>`: a restricted relational expression over account metadata,
      Borsh-located data fields, and named instruction arguments. Fails closed — an expression
      that cannot be evaluated is NOT satisfied (see `evalExpr`). -/
  | expr           (e : Expr)
  deriving Inhabited

/-- Whether a constraint is the `mut` (writable) marker. A constructor test rather than full
    `DecidableEq Constraint` — `Constraint` carries `ByteArray` payloads that lack
    `DecidableEq`, so we test the single constructor the distinct-mut-key check cares about. -/
def Constraint.isMut : Constraint → Bool
  | .mut => true
  | _    => false

/-- Account wrapper types; each implies certain base constraints. -/
inductive AccountType where
  | account          (typeName : String) (layout : Ty) (programId : Pubkey)
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

/-- Locate a field at a nested PATH inside this account's data. Offsets are measured from the
    start of `data`, so the walk begins at 8 — past the Anchor discriminator. Non-`account`
    wrappers have no modelled layout and yield `none`, which fails closed.

    The path generalisation exists for `Operand.field`, whose expressions may reach into nested
    structs (`vault.inner.amount`); `locate` already walks a path, so this is a pure widening
    of the Task 6 entry point rather than new machinery. -/
def AccountType.locateField' : AccountType → List String → ByteArray → Option (Nat × Ty)
  | .account _ layout _, path, data => locate layout path data 8
  | _, _, _ => none

/-- Single-name `locateField`, kept as the `has_one` entry point. Defined THROUGH `locateField'`
    so the two can never drift: `has_one` and `constraint = <expr>` must agree byte-for-byte on
    where a field lives. -/
def AccountType.locateField (t : AccountType) (name : String) (data : ByteArray) :
    Option (Nat × Ty) := t.locateField' [name] data

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
  /-- Declared `#[instruction(...)]` arguments in order, as a Borsh field list over
      `Ctx.instrData`. Defaults to `[]` so existing literals keep compiling. -/
  instrArgs : List (String × Ty) := []
  deriving Inhabited

/-- Find a declared field by name. -/
def AccountsStruct.fieldNamed (s : AccountsStruct) (name : String) : Option AccountField :=
  s.fields.find? (·.name == name)

end VerifiedAnchor
