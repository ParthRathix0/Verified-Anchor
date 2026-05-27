namespace VerifiedAnchor

/-- A Solana public key: 32 bytes. -/
abbrev Pubkey := Vector UInt8 32

/-- Account balance in lamports. -/
abbrev Lamports := UInt64

namespace Pubkey

/-- The all-zero pubkey (also the System Program id placeholder for examples). -/
def zero : Pubkey := Vector.replicate 32 0

/-- Build a pubkey from a list of bytes, padding with 0 / truncating to exactly 32.
    Total by construction — the length proof is discharged by `simp`. -/
def ofBytes (bs : List UInt8) : Pubkey :=
  ⟨((List.range 32).map (fun i => bs.getD i 0)).toArray, by simp⟩

end Pubkey

/-- DecidableEq is needed everywhere downstream. -/
example : DecidableEq Pubkey := inferInstance

#guard Pubkey.zero == Pubkey.zero
end VerifiedAnchor
