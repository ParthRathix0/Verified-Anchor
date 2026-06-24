import VerifiedAnchor.Codegen.Generated

namespace VerifiedAnchor

/-- The key bridge: `allB` agrees with `satisfiesSome` of the `= true` predicate. -/
theorem Option.allB_iff {α} (o : Option α) (p : α → Bool) :
    o.allB p = true ↔ o.satisfiesSome (fun a => p a = true) := by
  cases o <;> simp [Option.allB, Option.satisfiesSome]

theorem genConstraint_signer_iff (s c idx f) :
    genConstraint s c idx f Constraint.signer = true ↔ satisfies s c idx f Constraint.signer := by
  simp only [genConstraint, satisfies]; exact Option.allB_iff _ _

theorem genConstraint_mut_iff (s c idx f) :
    genConstraint s c idx f Constraint.mut = true ↔ satisfies s c idx f Constraint.mut := by
  simp only [genConstraint, satisfies]; exact Option.allB_iff _ _

theorem genConstraint_owner_iff (s c idx f e) :
    genConstraint s c idx f (Constraint.owner e) = true ↔ satisfies s c idx f (Constraint.owner e) := by
  simp only [genConstraint, satisfies, Option.allB_iff, decide_eq_true_iff]

theorem genConstraint_discriminator_iff (s c idx f d) :
    genConstraint s c idx f (Constraint.discriminator d) = true ↔ satisfies s c idx f (Constraint.discriminator d) := by
  simp only [genConstraint, satisfies, Option.allB_iff, decide_eq_true_iff]

theorem genConstraint_executable_iff (s c idx f) :
    genConstraint s c idx f Constraint.executable = true ↔ satisfies s c idx f Constraint.executable := by
  simp only [genConstraint, satisfies]; exact Option.allB_iff _ _

theorem genConstraint_address_iff (s c idx f e) :
    genConstraint s c idx f (Constraint.address e) = true ↔ satisfies s c idx f (Constraint.address e) := by
  simp only [genConstraint, satisfies, Option.allB_iff, decide_eq_true_iff]

theorem genConstraint_hasOne_iff (s c idx f field) :
    genConstraint s c idx f (Constraint.hasOne field) = true ↔ satisfies s c idx f (Constraint.hasOne field) := by
  simp only [genConstraint, genHasOne, satisfies, Option.allB_iff, decide_eq_true_iff]

theorem bumpMatchesB_iff (b : BumpSpec) (x : UInt8) :
    bumpMatchesB b x = true ↔ bumpMatches b x := by
  cases b with
  | declared db => simp [bumpMatchesB, bumpMatches]
  | canonical   => simp [bumpMatchesB, bumpMatches]
  | stored off  => simp [bumpMatchesB, bumpMatches]

theorem genConstraint_seeds_iff (s c idx f ss b program) :
    genConstraint s c idx f (Constraint.seeds ss b program) = true
      ↔ satisfies s c idx f (Constraint.seeds ss b program) := by
  -- The `seeds::program` override is just `program.getD s.programId` in both sides — definitional,
  -- so the proof is unchanged. Split on the bump: canonical/declared use the `findProgramAddress`
  -- + `bumpMatches` form; the `.stored` opt-in uses the byte lookup + `createProgramAddress` form.
  cases b with
  | declared db =>
      simp only [genConstraint, genSeeds, satisfies, Option.allB_iff, Bool.and_eq_true,
        decide_eq_true_iff, bumpMatchesB_iff]
  | canonical =>
      simp only [genConstraint, genSeeds, satisfies, Option.allB_iff, Bool.and_eq_true,
        decide_eq_true_iff, bumpMatchesB_iff]
  | stored off =>
      simp only [genConstraint, genSeeds, satisfies, Option.allB_iff, decide_eq_true_iff]

/-- Constraint kinds M3's generated validator handles. -/
def isM3Constraint : Constraint → Bool
  | .signer | .mut | .owner _ | .hasOne _ | .discriminator _ => true
  | _ => false

/-- The M3 subset: every field's (implied ++ explicit) constraints are M3 validation
    constraints. Typed `.account` is allowed (its implied owner+discriminator are M3). -/
def M3Subset (s : AccountsStruct) : Prop :=
  ∀ f ∈ s.fields, ∀ k ∈ (f.ty.impliedConstraints ++ f.constraints), isM3Constraint k = true

instance (s : AccountsStruct) : Decidable (M3Subset s) := by unfold M3Subset; infer_instance

theorem genConstraint_iff_satisfies_M3 (s c idx f k) (hk : isM3Constraint k = true) :
    genConstraint s c idx f k = true ↔ satisfies s c idx f k := by
  cases k with
  | signer        => exact genConstraint_signer_iff s c idx f
  | «mut»         => exact genConstraint_mut_iff s c idx f
  | owner e       => exact genConstraint_owner_iff s c idx f e
  | hasOne field  => exact genConstraint_hasOne_iff s c idx f field
  | discriminator d => exact genConstraint_discriminator_iff s c idx f d
  | _             => simp [isM3Constraint] at hk

/-- Constraint kinds M4's generated validator handles (M3 + seeds + the `Program<P>` /
    `SystemAccount` base checks `executable` and `address`). -/
def isM4Constraint : Constraint → Bool
  | .signer | .mut | .owner _ | .hasOne _ | .discriminator _ | .seeds _ _ _
  | .executable | .address _ => true
  | _ => false

/-- The M4 subset: every field's (implied ++ explicit) constraints are M4 validation
    constraints. Admits typed `.account` (implied owner+discriminator) AND `.seeds`. -/
def M4Subset (s : AccountsStruct) : Prop :=
  ∀ f ∈ s.fields, ∀ k ∈ (f.ty.impliedConstraints ++ f.constraints), isM4Constraint k = true

instance (s : AccountsStruct) : Decidable (M4Subset s) := by unfold M4Subset; infer_instance

/-- Dispatcher: under M4, the generated check of any constraint agrees with `satisfies`. -/
theorem genConstraint_iff_satisfies_M4 (s c idx f k) (hk : isM4Constraint k = true) :
    genConstraint s c idx f k = true ↔ satisfies s c idx f k := by
  cases k with
  | signer          => exact genConstraint_signer_iff s c idx f
  | «mut»           => exact genConstraint_mut_iff s c idx f
  | owner e         => exact genConstraint_owner_iff s c idx f e
  | hasOne field    => exact genConstraint_hasOne_iff s c idx f field
  | discriminator d => exact genConstraint_discriminator_iff s c idx f d
  | seeds ss b program => exact genConstraint_seeds_iff s c idx f ss b program
  | executable      => exact genConstraint_executable_iff s c idx f
  | address e       => exact genConstraint_address_iff s c idx f e
  | _               => simp [isM4Constraint] at hk

theorem genFieldValidate_iff (s c idx f)
    (hcons : ∀ k ∈ (f.ty.impliedConstraints ++ f.constraints), isM4Constraint k = true) :
    genFieldValidate s c idx f = true ↔ fieldValidates s c idx f := by
  unfold genFieldValidate fieldValidates
  rw [List.all_eq_true]
  constructor
  · intro hall k hk; exact (genConstraint_iff_satisfies_M4 s c idx f k (hcons k hk)).mp (hall k hk)
  · intro hall k hk; exact (genConstraint_iff_satisfies_M4 s c idx f k (hcons k hk)).mpr (hall k hk)

/-! ## Distinct mutable keys (M8.4): the struct-level conjunct

The third `validates` component. `isMutFieldB`/`exemptPairB` are Bool mirrors of the Props
`isMutField`/`exemptPair`; `distinctMutKeysB_iff` lifts them through the nested `List.all`
into `distinctMutKeys`, exactly the `List.all_eq_true` pattern the per-field arm uses. -/

/-- `isMutFieldB` is definitionally the `= true` form of `isMutField`. -/
theorem isMutFieldB_iff (f : AccountField) : isMutFieldB f = true ↔ isMutField f := by
  simp only [isMutFieldB, isMutField]

/-- The `= false` form: a field is NOT mut iff its Bool mirror is false. -/
theorem isMutFieldB_false_iff (f : AccountField) : isMutFieldB f = false ↔ ¬ isMutField f := by
  rw [← isMutFieldB_iff]; simp

/-- `exemptPairB` (BEq-`contains` + `||`) agrees with `exemptPair` (`∈` + `∨`). -/
theorem exemptPairB_iff (fi fj : AccountField) :
    exemptPairB fi fj = true ↔ exemptPair fi fj := by
  simp only [exemptPairB, exemptPair, Bool.or_eq_true, List.contains_iff_mem]

/-- The negated-guard form used in `distinctMutKeysB`'s `!(… && !exemptPairB …)` encoding:
    `!exemptPairB = false` (i.e. the pair IS exempt) iff `exemptPair`. -/
theorem exemptPairB_false_iff (fi fj : AccountField) :
    (!exemptPairB fi fj) = false ↔ exemptPair fi fj := by
  rw [Bool.not_eq_false', exemptPairB_iff]

/-- THE DISTINCT-MUT-KEY BRIDGE: the Bool struct-level check agrees with its Prop contract.
    Pushes the two `List.all`s through `List.all_eq_true`, distributes the `!(guards)` over the
    `&&` (De Morgan), and rewrites each Bool guard to its Prop via `isMutFieldB_false_iff`,
    `exemptPairB_false_iff`, `decide`, and `Option.allB_iff`. What remains is the
    `(¬lt ∨ ¬mut ∨ ¬mut ∨ exempt) ∨ keys≠`  ↔  `lt → mut → mut → ¬exempt → keys≠` shuffle. -/
theorem distinctMutKeysB_iff (s : AccountsStruct) (c : Ctx) :
    distinctMutKeysB s c = true ↔ distinctMutKeys s c := by
  unfold distinctMutKeysB distinctMutKeys
  simp only [List.all_eq_true, Bool.or_eq_true, Bool.not_and, Bool.not_eq_true',
    decide_eq_false_iff_not, decide_eq_true_eq, Option.allB_iff,
    isMutFieldB_false_iff, exemptPairB_false_iff]
  constructor
  · intro h p hp q hq hlt hmp hmq hnex
    rcases h p hp q hq with hg | hkeys
    · rcases hg with ((hlt' | hmp') | hmq') | hex'
      · exact absurd hlt hlt'
      · exact absurd hmp hmp'
      · exact absurd hmq hmq'
      · exact absurd hex' hnex
    · exact hkeys
  · intro h p hp q hq
    by_cases hlt : p.2 < q.2
    · by_cases hmp : isMutField p.1
      · by_cases hmq : isMutField q.1
        · by_cases hex : exemptPair p.1 q.1
          · exact Or.inl (Or.inr hex)
          · exact Or.inr (h p hp q hq hlt hmp hmq hex)
        · exact Or.inl (Or.inl (Or.inr hmq))
      · exact Or.inl (Or.inl (Or.inl (Or.inr hmp)))
    · exact Or.inl (Or.inl (Or.inl (Or.inl hlt)))

/-- THE M4 THEOREM: the generated validator agrees with the M1 contract for every struct in
    the M4 subset. Threads three conjuncts: wellformedness (`decide`), the struct-level
    distinct-mut-key check (`distinctMutKeysB_iff`), and per-field validation (`List.all`). -/
theorem genValidate_sound (s : AccountsStruct) (c : Ctx) (h : M4Subset s) :
    genValidate s c = true ↔ validates s c := by
  unfold genValidate validates
  rw [Bool.and_eq_true, Bool.and_eq_true, decide_eq_true_iff, distinctMutKeysB_iff]
  constructor
  · rintro ⟨⟨hwf, hdist⟩, hall⟩
    refine ⟨hwf, hdist, ?_⟩
    rw [List.all_eq_true] at hall
    intro p hp
    have hmemf : p.1 ∈ s.fields := List.fst_mem_of_mem_zipIdx hp
    exact (genFieldValidate_iff s c p.2 p.1 (h p.1 hmemf)).mp (hall p hp)
  · rintro ⟨hwf, hdist, hall⟩
    refine ⟨⟨hwf, hdist⟩, ?_⟩
    rw [List.all_eq_true]
    intro p hp
    have hmemf : p.1 ∈ s.fields := List.fst_mem_of_mem_zipIdx hp
    exact (genFieldValidate_iff s c p.2 p.1 (h p.1 hmemf)).mpr (hall p hp)

end VerifiedAnchor
