import VerifiedAnchor.Constraints.Context

/-! # Semantics of the `constraint = <expr>` sublanguage.

    The `Operand`/`Cmp`/`Expr` DATATYPES live in `Constraints/Ast.lean`, not here: `Constraint`
    has an `expr` constructor, so `Ast.lean` needs the syntax, while the evaluator below needs
    `Ctx` from `Context.lean`, which itself imports `Ast.lean`. Keeping only the semantics in
    this file breaks the would-be cycle `Ast → Expr → Context → Ast`. -/

namespace VerifiedAnchor

/-- Resolve an operand to a value. `none` = could not evaluate; every caller treats that as
    "constraint not satisfied", so the sublanguage fails CLOSED. Note there is no arithmetic:
    an operand is a literal, a piece of account metadata, a Borsh-located data field, or a
    named instruction argument — nothing that can overflow. -/
def evalOperand (s : AccountsStruct) (c : Ctx) : Operand → Option Value
  | .lit v => some v
  | .field i path => do
      let a ← Ctx.atField s c i
      let f ← s.fields[i]?
      let (off, t) ← f.ty.locateField' path a.data
      readVal t a.data off
  | .key i        => do let a ← Ctx.atField s c i; pure (.key a.key)
  | .owner i      => do let a ← Ctx.atField s c i; pure (.key a.owner)
  | .lamports i   => do let a ← Ctx.atField s c i; pure (.nat a.lamports.toNat)
  | .dataLen i    => do let a ← Ctx.atField s c i; pure (.nat a.data.size)
  | .isSigner i   => do let a ← Ctx.atField s c i; pure (.bool a.isSigner)
  | .isWritable i => do let a ← Ctx.atField s c i; pure (.bool a.isWritable)
  | .executable i => do let a ← Ctx.atField s c i; pure (.bool a.executable)
  | .instrArg n   => do
      let (off, t) ← locate (Ty.struct s.instrArgs) [n] c.instrData 0
      readVal t c.instrData off

/-- The mathematical integer a NUMERIC `Value` denotes, or `none` for a non-numeric one.

    `nat` and `int` are two ENCODINGS of one number line, not two incomparable universes. The
    old `evalCmp` refused to compare across them, on the grounds that `-1 : i64` and
    `18446744073709551615 : u64` share their bytes so any coercion would silently pick a sign
    convention. That argument is true at DECODE time and irrelevant here: `readVal` has already
    consulted the declared `Ty` and picked the sign, so by the time `evalCmp` runs it is holding
    two distinct, unambiguous mathematical integers. Comparing them in unbounded `Int` is exact
    and picks nothing. Do not re-narrow this on the old reasoning.

    Note this is deliberately a helper used ONLY by `evalCmp`. `Value`'s derived
    `DecidableEq`/`BEq` is left alone — `has_one` and the discriminator checks depend on
    constructor-sensitive equality, and widening it there would change unrelated code. -/
def Value.toInt? : Value → Option Int
  | .nat n => some (n : Int)
  | .int i => some i
  | _      => none

/-- Apply a comparison.

    NUMERIC pairs — `nat`/`nat`, `int`/`int`, AND the mixed `nat`/`int` pairings — compare as
    mathematical integers via `Value.toInt?`. The mixed case is not a convenience: without it,
    `delta != 0` on an `i64` field was a TAUTOLOGY (`int (-1) != nat 0` is `true` under
    constructor equality for every value of `delta`, including `0`), i.e. a security check the
    developer wrote and the model silently disabled. The `eq` direction was the mirror-image
    brick. A constant-valued guard that always ACCEPTS survives every happy-path test and
    surfaces only as an exploit, which is why this is a fix rather than a documented gap.

    NON-numeric pairs keep the old behaviour exactly: `eq`/`ne` stay TOTAL over `Value` (so
    `key k == bool b` is simply `false` — "is this the same value" is always answerable), while
    the four orderings yield `none` rather than `false`, so a type-confused comparison such as
    `key < nat` REJECTS rather than silently passing. -/
def evalCmp (op : Cmp) (a b : Value) : Option Bool :=
  match a.toInt?, b.toInt? with
  | some x, some y =>
      some <|
        match op with
        | .eq => x == y
        | .ne => x != y
        | .lt => x < y
        | .le => x ≤ y
        | .gt => x > y
        | .ge => x ≥ y
  | _, _ =>
      match op with
      | .eq => some (a == b)
      | .ne => some (a != b)
      | _   => none

/-- Evaluate an expression. `none` propagates: any unevaluable subterm makes the whole
    expression unevaluable, which the contract reads as "not satisfied". `and`/`or` are
    deliberately STRICT — no short-circuit — so that `false && <unevaluable>` is `none`
    rather than `some false`. Short-circuiting would let a malformed operand hide behind a
    cheap literal and make the two sides of the soundness bridge disagree on which subterms
    were ever looked at. -/
def evalExpr (s : AccountsStruct) (c : Ctx) : Expr → Option Bool
  | .cmp op l r => do
      let a ← evalOperand s c l
      let b ← evalOperand s c r
      evalCmp op a b
  | .and l r => do let a ← evalExpr s c l; let b ← evalExpr s c r; pure (a && b)
  | .or l r  => do let a ← evalExpr s c l; let b ← evalExpr s c r; pure (a || b)
  | .not e   => do let a ← evalExpr s c e; pure (!a)
  | .truthy o => do
      let v ← evalOperand s c o
      match v with
      | .bool b => pure b
      | _ => none

end VerifiedAnchor
