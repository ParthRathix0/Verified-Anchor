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

/-- Apply a comparison. `eq`/`ne` are total over `Value` (the derived `DecidableEq` compares
    constructors too, so `nat 1 == int 1` is simply `false`). Ordering is defined ONLY for
    like-typed numeric pairs; every other pairing yields `none` rather than `false`, so a
    type-confused comparison REJECTS rather than silently passing. -/
def evalCmp : Cmp → Value → Value → Option Bool
  | .eq, a, b => some (a == b)
  | .ne, a, b => some (a != b)
  | .lt, .nat a, .nat b => some (a < b)
  | .le, .nat a, .nat b => some (a ≤ b)
  | .gt, .nat a, .nat b => some (a > b)
  | .ge, .nat a, .nat b => some (a ≥ b)
  | .lt, .int a, .int b => some (a < b)
  | .le, .int a, .int b => some (a ≤ b)
  | .gt, .int a, .int b => some (a > b)
  | .ge, .int a, .int b => some (a ≥ b)
  | _, _, _ => none

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
