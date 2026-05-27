import VerifiedAnchor.Solana.Crypto
import VerifiedAnchor.Solana.Account

namespace VerifiedAnchor

/-- Anchor's 8-byte account discriminator: first 8 bytes of sha256("account:<Name>"). -/
def accountDiscriminator (name : String) : ByteArray :=
  (sha256 ("account:" ++ name).toUTF8).extract 0 8

/-- Two byte arrays agree on their first `n` bytes. -/
def bytesAgreePrefix (x y : ByteArray) (n : Nat) : Prop :=
  ∀ i, i < n → x[i]? = y[i]?

instance (x y : ByteArray) (n : Nat) : Decidable (bytesAgreePrefix x y n) := by
  unfold bytesAgreePrefix
  exact Nat.decidableBallLT n (fun i _ => x[i]? = y[i]?)

/-- An account's data begins with the given 8-byte discriminator. -/
def hasDiscriminator (a : AccountInfo) (d : ByteArray) : Prop :=
  bytesAgreePrefix a.data d 8

instance (a : AccountInfo) (d : ByteArray) : Decidable (hasDiscriminator a d) :=
  inferInstanceAs (Decidable (bytesAgreePrefix a.data d 8))

end VerifiedAnchor
