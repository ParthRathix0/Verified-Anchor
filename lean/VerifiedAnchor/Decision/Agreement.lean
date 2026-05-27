import VerifiedAnchor.Decision.Check

namespace VerifiedAnchor

/-- The executable checker agrees with the declarative contract. -/
theorem validates_iff_validatesBool (s : AccountsStruct) (c : Ctx) :
    validates s c ↔ validatesBool s c = true := by
  unfold validatesBool
  exact (decide_eq_true_iff).symm

/-- Corollary: a true checker result is a proof of the contract. -/
theorem validatesBool_sound (s : AccountsStruct) (c : Ctx)
    (h : validatesBool s c = true) : validates s c :=
  (validates_iff_validatesBool s c).mpr h

/-- Corollary: the checker never rejects a validating context. -/
theorem validatesBool_complete (s : AccountsStruct) (c : Ctx)
    (h : validates s c) : validatesBool s c = true :=
  (validates_iff_validatesBool s c).mp h

end VerifiedAnchor
