import VerifiedAnchor.Solana.Borsh.Ty

namespace VerifiedAnchor

/-- `Vault { bump : u8, authority : Pubkey }` after the 8-byte discriminator.
    Bytes: 8 disc + 1 bump + 32 authority = 41. -/
private def vaultTy : Ty := .struct [("bump", .u8), ("authority", .pubkey)]

private def vaultBytes : ByteArray :=
  ⟨(Array.replicate 8 (0 : UInt8)) ++ #[(7 : UInt8)] ++ (Array.replicate 32 (3 : UInt8))⟩

/-- `Named { label : String, owner : Pubkey }` with label = "hi".
    Bytes: 8 disc + 4 len + 2 utf8 + 32 owner. -/
private def namedTy : Ty := .struct [("label", .string), ("owner", .pubkey)]

private def namedBytes : ByteArray :=
  ⟨(Array.replicate 8 (0 : UInt8))
    ++ #[(2 : UInt8), 0, 0, 0] ++ #[(104 : UInt8), 105]
    ++ (Array.replicate 32 (9 : UInt8))⟩

/-- Little-endian unsigned read of `w` bytes at `off`. `none` if out of bounds. -/
def readUIntLE (data : ByteArray) (off : Nat) (w : Nat) : Option Nat :=
  if off + w ≤ data.size then
    some ((List.range w).foldl (fun acc i => acc + (data.get! (off + i)).toNat * 256 ^ i) 0)
  else none

/-- Total width of `count` consecutive elements whose individual widths are produced by
    `elemWidth off`. Recurses structurally on `count`, and takes the element-width function as a
    parameter rather than living inside the `encodedWidth` mutual block below: with `vecWidth`
    *in* that block there is no argument all three functions shrink on (`encodedWidth` shrinks a
    `Ty`, `vecWidth` a `Nat`), so Lean falls back to well-founded recursion — and well-founded
    definitions are opaque to the kernel's `decide`, which downstream proof obligations rely on.
    Lifting the loop out keeps every definition here structural, hence kernel-reducible. -/
def vecWidthFrom (elemWidth : Nat → Option Nat) (off : Nat) (count : Nat) (acc : Nat) :
    Option Nat :=
  match count with
  | 0 => some acc
  | n + 1 => do
      let w ← elemWidth off
      vecWidthFrom elemWidth (off + w) n (acc + w)

mutual
  /-- Encoded width of the value starting at `off`. Unlike `Ty.byteSize`, which is a static
      property of the type, this consults the *bytes*: a `string`/`vec` width is only knowable
      after reading its u32 length prefix, and an `option`'s only after its 1-byte tag. This is
      what lets `locate` step over a variable-length field to reach the one behind it. -/
  def encodedWidth (t : Ty) (data : ByteArray) (off : Nat) : Option Nat :=
    match t with
    | .string => do
        let n ← readUIntLE data off 4
        if off + 4 + n ≤ data.size then pure (4 + n) else none
    | .vec e => do
        let n ← readUIntLE data off 4
        vecWidthFrom (fun o => encodedWidth e data o) (off + 4) n 4
    | .option e => do
        let tag ← readUIntLE data off 1
        if tag == 0 then pure 1
        else do let w ← encodedWidth e data (off + 1); pure (1 + w)
    | .struct fs => fieldsWidth fs data off 0
    | other => Ty.byteSize other
  termination_by structural t

  /-- Total width of a field list laid out consecutively from `off`. The `off` parameter
      advances as well as the accumulator because a later field's width can depend on the bytes
      that earlier fields occupy. -/
  def fieldsWidth : List (String × Ty) → ByteArray → Nat → Nat → Option Nat
    | [], _, _, acc => some acc
    | (_, t) :: rest, data, off, acc => do
        let w ← encodedWidth t data off
        fieldsWidth rest data (off + w) (acc + w)
  termination_by structural fields => fields
end

mutual
  /-- Walk `path` through `t` starting at byte offset `start`, stepping over variable-length
      fields by consuming their length prefixes. Returns the target's offset and type.

      `start` is measured from the start of `data`, not from the struct: callers pass 8 to skip
      Anchor's discriminator. Returning the type alongside the offset is what lets the caller
      `readVal` at the right width without re-deriving the field's type. -/
  def locate (t : Ty) (path : List String) (data : ByteArray) (start : Nat) :
      Option (Nat × Ty) :=
    match path with
    | [] => some (start, t)
    | name :: rest =>
        match t with
        | .struct fs => locateInFields fs name rest data start
        | _ => none
  termination_by structural t

  /-- Scan a struct's fields for `name`, advancing `off` past each non-matching field by its
      *encoded* (byte-dependent) width. Fails closed with `none` on an unknown field name, and
      also whenever a skipped field's width cannot be determined from the buffer. -/
  def locateInFields : List (String × Ty) → String → List String → ByteArray → Nat →
      Option (Nat × Ty)
    | [], _, _, _, _ => none
    | (fname, fty) :: more, name, rest, data, off =>
        if fname == name then locate fty rest data off
        else do
          let w ← encodedWidth fty data off
          locateInFields more name rest data (off + w)
  termination_by structural fields => fields
end

/-- Decode one scalar at `off`. Aggregates are not values; they yield `none`. -/
def readVal (t : Ty) (data : ByteArray) (off : Nat) : Option Value :=
  match t with
  | .u8 => (readUIntLE data off 1).map .nat
  | .u16 => (readUIntLE data off 2).map .nat
  | .u32 => (readUIntLE data off 4).map .nat
  | .u64 => (readUIntLE data off 8).map .nat
  | .u128 => (readUIntLE data off 16).map .nat
  | .i8 => (readUIntLE data off 1).map (fun n =>
      .int (if n < 128 then (n : Int) else (n : Int) - 256))
  | .i16 => (readUIntLE data off 2).map (fun n =>
      .int (if n < 32768 then (n : Int) else (n : Int) - 65536))
  | .i32 => (readUIntLE data off 4).map (fun n =>
      .int (if n < 2 ^ 31 then (n : Int) else (n : Int) - 2 ^ 32))
  | .i64 => (readUIntLE data off 8).map (fun n =>
      .int (if n < 2 ^ 63 then (n : Int) else (n : Int) - 2 ^ 64))
  | .i128 => (readUIntLE data off 16).map (fun n =>
      .int (if n < 2 ^ 127 then (n : Int) else (n : Int) - 2 ^ 128))
  | .bool => (readUIntLE data off 1).map (fun n => .bool (n != 0))
  | .pubkey =>
      if off + 32 ≤ data.size then
        some (.key (Pubkey.ofBytes ((data.extract off (off + 32)).toList)))
      else none
  | .array _ _ | .string | .vec _ | .option _ | .struct _ => none

-- offsets are measured from the start of `data`, so the walk starts at 8 (past the discriminator)
#guard (locate vaultTy ["bump"] vaultBytes 8).map (·.1) == some 8
#guard (locate vaultTy ["authority"] vaultBytes 8).map (·.1) == some 9
#guard (locate vaultTy ["missing"] vaultBytes 8) == none

-- THE POINT OF locate: a field positioned after a variable-length String is still found
#guard (locate namedTy ["owner"] namedBytes 8).map (·.1) == some 14

#guard readVal .u8 vaultBytes 8 == some (.nat 7)
#guard readVal .u64 (⟨#[1,0,0,0,0,0,0,0]⟩ : ByteArray) 0 == some (.nat 1)
#guard readVal .u64 (⟨#[0,1,0,0,0,0,0,0]⟩ : ByteArray) 0 == some (.nat 256)
#guard readVal .bool (⟨#[0]⟩ : ByteArray) 0 == some (.bool false)
#guard readVal .bool (⟨#[1]⟩ : ByteArray) 0 == some (.bool true)
#guard readVal .u64 (⟨#[1,2,3]⟩ : ByteArray) 0 == none   -- out of bounds fails closed

/- The guards above run at elaboration time, which is a weaker check than what Tasks 6/10/11
   need: those discharge obligations over `locate`/`encodedWidth` by *kernel* reduction. A
   well-founded definition elaborates and `#guard`s perfectly happily while remaining opaque to
   `decide` (its `Acc.rec` never unfolds) — the first draft of this file did exactly that. The
   `by decide` / `by rfl` examples below are therefore load-bearing regression tests: they fail
   the build if anyone reintroduces well-founded (or `partial`) recursion here. -/

example : (locate vaultTy ["authority"] vaultBytes 8).map (·.1) = some 9 := by decide
example : (locate namedTy ["owner"] namedBytes 8).map (·.1) = some 14 := by decide
example : encodedWidth .string namedBytes 8 = some 6 := by decide
example : encodedWidth (.vec .u8) (⟨#[3,0,0,0,1,2,3]⟩ : ByteArray) 0 = some 7 := by decide
example : encodedWidth (.option .u64) (⟨#[0]⟩ : ByteArray) 0 = some 1 := by decide
example : encodedWidth (.option .u64) (⟨#[1,0,0,0,0,0,0,0,0]⟩ : ByteArray) 0 = some 9 := by decide
example : readVal .u8 vaultBytes 8 = some (.nat 7) := by decide
example : readVal .u64 (⟨#[1,2,3]⟩ : ByteArray) 0 = none := by decide
-- `rfl` is strictly stronger than `decide`: it forces whnf of the walk itself.
example : encodedWidth (.vec .u8) (⟨#[2,0,0,0,1,2]⟩ : ByteArray) 0 = some 6 := by rfl
example : (locate namedTy ["owner"] namedBytes 8).map (·.1) = some 14 := by rfl

end VerifiedAnchor
