import VerifiedAnchor.Solana.Pubkey

namespace VerifiedAnchor

/-- SHA-256. Uninterpreted: we model its interface, not its bit-level behavior. -/
opaque sha256 : ByteArray → ByteArray

/-- SHA-256 always returns 32 bytes. The only crypto fact M1 relies on. -/
axiom sha256_size (b : ByteArray) : (sha256 b).size = 32

/-- Read the SHA-256 output as a `Pubkey`. Uses the total `ofBytes` constructor so no
    length proof against `sha256_size` is needed (it pads/truncates to 32). -/
def sha256Pubkey (b : ByteArray) : Pubkey :=
  Pubkey.ofBytes (sha256 b).toList

/-- ed25519 on-curve test. Uninterpreted. -/
opaque isOnCurve : Pubkey → Bool

/-- Solana's create_program_address: hash seeds ++ programId ++ marker, fail if on-curve. -/
def createProgramAddress (seeds : List ByteArray) (programId : Pubkey) : Option Pubkey :=
  let marker := "ProgramDerivedAddress".toUTF8
  let input := (seeds.foldl (· ++ ·) ByteArray.empty)
                 ++ ByteArray.mk programId.toArray ++ marker
  let candidate := sha256Pubkey input
  if isOnCurve candidate then none else some candidate

/-- Solana's find_program_address: iterate bump 255→0, first off-curve hit wins. -/
def findProgramAddress (seeds : List ByteArray) (programId : Pubkey) :
    Option (Pubkey × UInt8) :=
  let rec go (bump : Nat) : Option (Pubkey × UInt8) :=
    match bump with
    | 0 => match createProgramAddress (seeds ++ [(⟨#[(0 : UInt8)]⟩ : ByteArray)]) programId with
           | some pk => some (pk, 0)
           | none => none
    | n+1 =>
      match createProgramAddress (seeds ++ [(⟨#[UInt8.ofNat (n+1)]⟩ : ByteArray)]) programId with
      | some pk => some (pk, UInt8.ofNat (n+1))
      | none => go n
  go 255

end VerifiedAnchor
