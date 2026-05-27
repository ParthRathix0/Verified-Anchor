import VerifiedAnchor.Solana.Pubkey

namespace VerifiedAnchor

/-- Maps a named `Pubkey` field of an account's deserialized struct to its byte offset
    (offset measured from the start of `data`, i.e. including the 8-byte discriminator). -/
abbrev FieldLayout := List (String × Nat)

def FieldLayout.offsetOf (l : FieldLayout) (name : String) : Option Nat :=
  (l.find? (·.1 == name)).map (·.2)

/-- Read a 32-byte Pubkey at `offset` in `data`, or `none` if out of bounds.
    Uses the total `ofBytes` constructor — no length proof needed. -/
def readPubkey (data : ByteArray) (offset : Nat) : Option Pubkey :=
  if offset + 32 ≤ data.size then
    some (Pubkey.ofBytes ((data.extract offset (offset + 32)).toList))
  else none

end VerifiedAnchor
