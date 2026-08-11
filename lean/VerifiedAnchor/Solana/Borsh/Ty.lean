import VerifiedAnchor.Solana.Pubkey

namespace VerifiedAnchor

/-- Borsh type descriptors for the fields of an account-data struct. Floats are deliberately
    absent: they are rare in account state and comparison-unsafe, so a constraint touching one
    falls to the reported escape hatch rather than being silently modelled. Enums are absent for
    the same reason (see the design spec's Risks section). -/
inductive Ty where
  | u8 | u16 | u32 | u64 | u128
  | i8 | i16 | i32 | i64 | i128
  | bool
  | pubkey
  | array  (elem : Ty) (n : Nat)
  | string
  | vec    (elem : Ty)
  | option (elem : Ty)
  | struct (fields : List (String × Ty))
  deriving Inhabited, Repr

/-- A decoded scalar. `bytes` carries a `List UInt8` rather than a `ByteArray` so `DecidableEq`
    derives — expression comparison needs it. -/
inductive Value where
  | nat   (n : Nat)
  | int   (i : Int)
  | bool  (b : Bool)
  | key   (p : Pubkey)
  | bytes (bs : List UInt8)
  deriving Inhabited, Repr, DecidableEq

/- `Ty` recurses through `List (String × Ty)` — a *nested* inductive. Lean 4's automatic
   `deriving DecidableEq` handler does not support nested inductives (confirmed empirically:
   it fails even on the simpler `List Ty` shape, with no `String` pairing involved), so the
   instance below is hand-written instead of derived. It follows the same shape the brief
   mandates for `byteSize`/`fieldsByteSize`: a `mutual` pair, one arm per type. Every branch
   below uses `cases ... with | ctor => ... | _ => ...` (never a bare term-mode `match _, _`
   wildcard) because only the `cases` tactic keeps each generated goal's constructors concrete
   enough for `injection` to close it; a term-level double wildcard was tried and does not
   retain that information. -/
mutual
  /-- Decidable equality for `Ty`, hand-written because `Ty` is a nested inductive (see the
      note above `Ty.decEq`'s `mutual` block). -/
  def Ty.decEq (a b : Ty) : Decidable (a = b) := by
    cases a with
    | u8 => cases b with | u8 => exact isTrue rfl | _ => exact isFalse (by intro h; injection h)
    | u16 => cases b with | u16 => exact isTrue rfl | _ => exact isFalse (by intro h; injection h)
    | u32 => cases b with | u32 => exact isTrue rfl | _ => exact isFalse (by intro h; injection h)
    | u64 => cases b with | u64 => exact isTrue rfl | _ => exact isFalse (by intro h; injection h)
    | u128 => cases b with | u128 => exact isTrue rfl | _ => exact isFalse (by intro h; injection h)
    | i8 => cases b with | i8 => exact isTrue rfl | _ => exact isFalse (by intro h; injection h)
    | i16 => cases b with | i16 => exact isTrue rfl | _ => exact isFalse (by intro h; injection h)
    | i32 => cases b with | i32 => exact isTrue rfl | _ => exact isFalse (by intro h; injection h)
    | i64 => cases b with | i64 => exact isTrue rfl | _ => exact isFalse (by intro h; injection h)
    | i128 => cases b with | i128 => exact isTrue rfl | _ => exact isFalse (by intro h; injection h)
    | bool => cases b with | bool => exact isTrue rfl | _ => exact isFalse (by intro h; injection h)
    | pubkey => cases b with | pubkey => exact isTrue rfl | _ => exact isFalse (by intro h; injection h)
    | string => cases b with | string => exact isTrue rfl | _ => exact isFalse (by intro h; injection h)
    | array e n => cases b with
        | array e2 n2 =>
            exact match Ty.decEq e e2, Nat.decEq n n2 with
              | isTrue he, isTrue hn => isTrue (by rw [he, hn])
              | isFalse he, _ => isFalse (by intro h; injection h with h1 h2; exact he h1)
              | _, isFalse hn => isFalse (by intro h; injection h with h1 h2; exact hn h2)
        | _ => exact isFalse (by intro h; injection h)
    | vec e => cases b with
        | vec e2 =>
            exact match Ty.decEq e e2 with
              | isTrue h => isTrue (by rw [h])
              | isFalse h => isFalse (by intro he; apply h; injection he)
        | _ => exact isFalse (by intro h; injection h)
    | option e => cases b with
        | option e2 =>
            exact match Ty.decEq e e2 with
              | isTrue h => isTrue (by rw [h])
              | isFalse h => isFalse (by intro he; apply h; injection he)
        | _ => exact isFalse (by intro h; injection h)
    | struct fs => cases b with
        | struct fs2 =>
            exact match Ty.decEqFields fs fs2 with
              | isTrue h => isTrue (by rw [h])
              | isFalse h => isFalse (by intro he; apply h; injection he)
        | _ => exact isFalse (by intro h; injection h)

  /-- Decidable equality for a `struct`'s field list, paired with `Ty.decEq` for the same
      nested-inductive reason. -/
  def Ty.decEqFields (a b : List (String × Ty)) : Decidable (a = b) :=
    match a, b with
    | [], [] => isTrue rfl
    | [], _ :: _ => isFalse (by intro h; injection h)
    | _ :: _, [] => isFalse (by intro h; injection h)
    | (n1, t1) :: r1, (n2, t2) :: r2 =>
        match String.decEq n1 n2, Ty.decEq t1 t2, Ty.decEqFields r1 r2 with
        | isTrue hn, isTrue ht, isTrue hr => isTrue (by rw [hn, ht, hr])
        | isFalse hn, _, _ => isFalse (by intro h; injection h with h1 h2; exact hn (by injection h1))
        | _, isFalse ht, _ => isFalse (by intro h; injection h with h1 h2; exact ht (by injection h1))
        | _, _, isFalse hr => isFalse (by intro h; injection h with h1 h2; exact hr h2)
end

instance : DecidableEq Ty := Ty.decEq

mutual
  /-- Encoded width in bytes, or `none` for variable-length types. -/
  def Ty.byteSize : Ty → Option Nat
    | .u8 | .i8 | .bool => some 1
    | .u16 | .i16 => some 2
    | .u32 | .i32 => some 4
    | .u64 | .i64 => some 8
    | .u128 | .i128 => some 16
    | .pubkey => some 32
    | .array e n => (Ty.byteSize e).map (· * n)
    | .string | .vec _ | .option _ => none
    | .struct fs => Ty.fieldsByteSize fs

  /-- Total width of a field list; `none` if any field is variable-length. -/
  def Ty.fieldsByteSize : List (String × Ty) → Option Nat
    | [] => some 0
    | (_, t) :: rest => do
        let a ← Ty.byteSize t
        let b ← Ty.fieldsByteSize rest
        pure (a + b)
end

#guard Ty.byteSize .u64 == some 8
#guard Ty.byteSize .pubkey == some 32
#guard Ty.byteSize .string == none
#guard Ty.byteSize (.vec .u8) == none
#guard Ty.byteSize (.option .u64) == none
#guard Ty.byteSize (.array .u32 4) == some 16
#guard Ty.byteSize (.struct [("a", .u8), ("b", .pubkey)]) == some 33
#guard Ty.byteSize (.struct [("a", .string), ("b", .u8)]) == none

end VerifiedAnchor
