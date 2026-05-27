import VerifiedAnchor.Solana.Pubkey

namespace VerifiedAnchor

/-- Faithful model of Solana's `AccountInfo` fields. -/
structure AccountInfo where
  key        : Pubkey
  lamports   : UInt64
  data       : ByteArray
  owner      : Pubkey
  rentEpoch  : UInt64
  isSigner   : Bool
  isWritable : Bool
  executable : Bool
  deriving Inhabited

/-- First `n` bytes of an account's data, or all of them if shorter. -/
def AccountInfo.dataPrefix (a : AccountInfo) (n : Nat) : ByteArray :=
  a.data.extract 0 (min n a.data.size)

end VerifiedAnchor
