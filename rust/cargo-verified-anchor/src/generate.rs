//! Turn collected specs into a Lean `check.lean` of per-struct `decide` obligations.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Validation,
    Lifecycle,
}

pub struct Spec {
    pub name: String,
    pub kind: Kind,
    pub lean_spec: String,
    /// Source text of the constraints running outside the proof (Task 13's escape hatch).
    /// Empty when the struct is fully proven.
    pub unproven: Vec<String>,
}

/// One obligation per struct: validation -> `M10Subset`, lifecycle -> `StructLifecycleWF`.
/// `M10Subset` is the M10 (Task 11) name; `M4Subset` remains as a reducible `abbrev` in Lean
/// for older emitted files / a published 0.3.x tool, but our own emitter now names it directly.
pub fn generate_check_lean(specs: &[Spec]) -> String {
    let mut out = String::from("import VerifiedAnchor\nopen VerifiedAnchor\n\ndef ownerPlaceholder : Pubkey := Pubkey.zero\n");
    let mut specs: Vec<&Spec> = specs.iter().collect();
    specs.sort_by(|a, b| a.name.cmp(&b.name)); // deterministic output
    for s in specs {
        let pred = match s.kind {
            Kind::Validation => "M10Subset",
            Kind::Lifecycle => "StructLifecycleWF",
        };
        let kind_str = match s.kind {
            Kind::Validation => "validation",
            Kind::Lifecycle => "lifecycle",
        };
        out.push_str(&format!(
            "\n-- {} ({})\nexample : {} ({}) := by decide\n",
            s.name, kind_str, pred, s.lean_spec
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_struct_emits_m10subset() {
        let specs = vec![Spec {
            name: "T".into(),
            kind: Kind::Validation,
            lean_spec: "SPEC_T".into(),
            unproven: vec![],
        }];
        let out = generate_check_lean(&specs);
        assert!(out.contains("import VerifiedAnchor"));
        assert!(out.contains("def ownerPlaceholder : Pubkey := Pubkey.zero"));
        assert!(out.contains("-- T (validation)\nexample : M10Subset (SPEC_T) := by decide"));
    }

    #[test]
    fn lifecycle_struct_emits_structlifecyclewf() {
        let specs = vec![Spec {
            name: "V".into(),
            kind: Kind::Lifecycle,
            lean_spec: "SPEC_V".into(),
            unproven: vec![],
        }];
        let out = generate_check_lean(&specs);
        assert!(out.contains("-- V (lifecycle)\nexample : StructLifecycleWF (SPEC_V) := by decide"));
    }

    #[test]
    fn output_is_sorted_by_name() {
        let specs = vec![
            Spec {
                name: "B".into(),
                kind: Kind::Validation,
                lean_spec: "SB".into(),
                unproven: vec![],
            },
            Spec {
                name: "A".into(),
                kind: Kind::Validation,
                lean_spec: "SA".into(),
                unproven: vec![],
            },
        ];
        let out = generate_check_lean(&specs);
        assert!(out.find("-- A (").unwrap() < out.find("-- B (").unwrap());
    }
}
