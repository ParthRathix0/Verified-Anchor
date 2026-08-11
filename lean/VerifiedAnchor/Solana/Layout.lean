import VerifiedAnchor.Solana.Pubkey

namespace VerifiedAnchor

/-- Read a 32-byte Pubkey at `offset` in `data`, or `none` if out of bounds.
    Uses the total `ofBytes` constructor — no length proof needed. -/
def readPubkey (data : ByteArray) (offset : Nat) : Option Pubkey :=
  if offset + 32 ≤ data.size then
    some (Pubkey.ofBytes ((data.extract offset (offset + 32)).toList))
  else none

end VerifiedAnchor
