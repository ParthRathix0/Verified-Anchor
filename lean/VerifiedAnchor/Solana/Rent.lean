import VerifiedAnchor.Solana.Pubkey

namespace VerifiedAnchor

/-- The rent-exempt minimum balance (in lamports) for an account holding `dataSize` bytes.

    OPAQUE BY DESIGN — an uninterpreted wall, exactly like `sha256` / `isOnCurve`. Solana's
    real `Rent::is_exempt` computes this from the cluster's live rent parameters
    (`lamports_per_byte_year`, `exemption_threshold`, the per-account storage overhead), none
    of which we model concretely. We therefore expose ONLY the interface — a monotone-looking
    `Nat → Lamports` map from data size to the required minimum — and prove the generated
    validator equals the contract SCHEMATICALLY over it (the soundness theorem is `∀` over this
    function's values, never reducing them).

    The correspondence between THIS opaque function and Solana's runtime `Rent::is_exempt` is
    cross-checked EMPIRICALLY by the litesvm runtime tests (an under-funded account is rejected
    on-chain, a rent-exempt account is accepted), NOT proven in Lean. Same honesty boundary as
    the `sha256` PDA wall. -/
opaque rentExemptMinimum : Nat → Lamports

end VerifiedAnchor
