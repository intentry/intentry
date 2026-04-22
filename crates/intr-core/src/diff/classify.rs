use crate::version::BumpKind;
use super::types::{Change, ChangeCategory, ChangeKind, DiffSummary};

// ---------------------------------------------------------------------------
// Semver bump heuristics
// ---------------------------------------------------------------------------
//
// Rules (in priority order — highest wins):
//
// 1. `output.schema` or `input.schema` **Removed** → MAJOR (breaking consumers)
// 2. `output.schema` or `input.schema` **Added**   → MINOR (additive, but may break strict clients)
// 3. `evals` **Removed**                           → MINOR
// 4. `model.*` changed + no schema change          → PATCH
// 5. Body (`SemanticTemplate`) changed             → PATCH
// 6. Only Metadata / Version / Cosmetic            → PATCH (or no bump)
//
// If there are zero semantic changes at all → PATCH (the default minimum).

/// Compute a [`DiffSummary`] from a slice of [`Change`] items.
pub fn classify(changes: &[Change]) -> DiffSummary {
    let mut semantic_changes: u32 = 0;
    let mut cosmetic_changes: u32 = 0;

    let mut schema_removed  = false;
    let mut schema_added    = false;
    let mut evals_removed   = false;

    for change in changes {
        match change.category {
            ChangeCategory::Cosmetic | ChangeCategory::Version => {
                cosmetic_changes += 1;
            }
            ChangeCategory::Metadata => {
                // Metadata changes count as cosmetic for bump purposes.
                cosmetic_changes += 1;
            }
            _ => {
                semantic_changes += 1;
            }
        }

        // Schema-removal check.
        if matches!(change.category, ChangeCategory::SemanticSchema)
            && matches!(change.kind, ChangeKind::Removed)
            && (change.path.starts_with("input") || change.path.starts_with("output"))
        {
            schema_removed = true;
        }

        // Schema-addition check.
        if matches!(change.category, ChangeCategory::SemanticSchema)
            && matches!(change.kind, ChangeKind::Added)
        {
            schema_added = true;
        }

        // Eval-removal check.
        if matches!(change.category, ChangeCategory::SemanticEvals)
            && matches!(change.kind, ChangeKind::Removed)
        {
            evals_removed = true;
        }
    }

    let is_breaking = schema_removed;

    let suggested_version_bump = if schema_removed {
        BumpKind::Major
    } else if schema_added || evals_removed {
        BumpKind::Minor
    } else {
        BumpKind::Patch
    };

    DiffSummary {
        semantic_changes,
        cosmetic_changes,
        is_breaking,
        suggested_version_bump,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::types::{Change, ChangeCategory, ChangeKind};

    fn change(category: ChangeCategory, kind: ChangeKind, path: &str) -> Change {
        Change {
            category,
            path: path.to_string(),
            kind,
            before: None,
            after: None,
            line_range: None,
        }
    }

    #[test]
    fn empty_changes_gives_patch() {
        let summary = classify(&[]);
        assert_eq!(summary.suggested_version_bump, BumpKind::Patch);
        assert!(!summary.is_breaking);
        assert_eq!(summary.semantic_changes, 0);
        assert_eq!(summary.cosmetic_changes, 0);
    }

    #[test]
    fn schema_removal_is_major_and_breaking() {
        let cs = vec![change(
            ChangeCategory::SemanticSchema,
            ChangeKind::Removed,
            "output.schema",
        )];
        let summary = classify(&cs);
        assert_eq!(summary.suggested_version_bump, BumpKind::Major);
        assert!(summary.is_breaking);
    }

    #[test]
    fn input_schema_removal_is_major() {
        let cs = vec![change(
            ChangeCategory::SemanticSchema,
            ChangeKind::Removed,
            "input.schema",
        )];
        let summary = classify(&cs);
        assert_eq!(summary.suggested_version_bump, BumpKind::Major);
    }

    #[test]
    fn schema_addition_is_minor_not_breaking() {
        let cs = vec![change(
            ChangeCategory::SemanticSchema,
            ChangeKind::Added,
            "output.schema",
        )];
        let summary = classify(&cs);
        assert_eq!(summary.suggested_version_bump, BumpKind::Minor);
        assert!(!summary.is_breaking);
    }

    #[test]
    fn evals_removal_is_minor() {
        let cs = vec![change(
            ChangeCategory::SemanticEvals,
            ChangeKind::Removed,
            "evals[test-1]",
        )];
        let summary = classify(&cs);
        assert_eq!(summary.suggested_version_bump, BumpKind::Minor);
        assert!(!summary.is_breaking);
    }

    #[test]
    fn model_change_only_is_patch() {
        let cs = vec![change(
            ChangeCategory::SemanticModel,
            ChangeKind::Modified,
            "model.temperature",
        )];
        let summary = classify(&cs);
        assert_eq!(summary.suggested_version_bump, BumpKind::Patch);
        assert!(!summary.is_breaking);
    }

    #[test]
    fn body_change_only_is_patch() {
        let cs = vec![change(
            ChangeCategory::SemanticTemplate,
            ChangeKind::Modified,
            "body[1:3]",
        )];
        let summary = classify(&cs);
        assert_eq!(summary.suggested_version_bump, BumpKind::Patch);
    }

    #[test]
    fn cosmetic_only_is_patch_zero_semantic() {
        let cs = vec![change(ChangeCategory::Cosmetic, ChangeKind::Modified, "body[1:1]")];
        let summary = classify(&cs);
        assert_eq!(summary.semantic_changes, 0);
        assert_eq!(summary.cosmetic_changes, 1);
        assert_eq!(summary.suggested_version_bump, BumpKind::Patch);
    }

    #[test]
    fn schema_removal_beats_schema_addition() {
        // Both removal and addition in the same diff → removal wins (major).
        let cs = vec![
            change(ChangeCategory::SemanticSchema, ChangeKind::Removed, "output.schema"),
            change(ChangeCategory::SemanticSchema, ChangeKind::Added,   "input.schema"),
        ];
        let summary = classify(&cs);
        assert_eq!(summary.suggested_version_bump, BumpKind::Major);
        assert!(summary.is_breaking);
    }

    #[test]
    fn multiple_semantic_changes_counted() {
        let cs = vec![
            change(ChangeCategory::SemanticTemplate, ChangeKind::Modified, "body[1:1]"),
            change(ChangeCategory::SemanticModel,    ChangeKind::Modified, "model.temperature"),
            change(ChangeCategory::Metadata,         ChangeKind::Modified, "description"),
        ];
        let summary = classify(&cs);
        assert_eq!(summary.semantic_changes, 2);
        assert_eq!(summary.cosmetic_changes, 1);
    }

    #[test]
    fn version_changes_are_cosmetic_for_bump() {
        let cs = vec![change(ChangeCategory::Version, ChangeKind::Modified, "version")];
        let summary = classify(&cs);
        assert_eq!(summary.semantic_changes, 0);
        assert_eq!(summary.suggested_version_bump, BumpKind::Patch);
    }
}
