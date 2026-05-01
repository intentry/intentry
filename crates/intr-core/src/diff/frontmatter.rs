use serde_json::Value;

use super::types::{Change, ChangeCategory, ChangeKind};

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Compare two `serde_yaml::Value` frontmatter blobs, emitting a [`Change`]
/// for every field that differs.
///
/// `from_raw` and `to_raw` are the raw YAML strings of the frontmatter blocks
/// (without the `---` fence delimiters). Passing `None` signals that the side
/// has no frontmatter (Tier-1 file).
pub fn diff_frontmatter(from_raw: Option<&str>, to_raw: Option<&str>) -> Vec<Change> {
    let from_val = parse_yaml(from_raw);
    let to_val = parse_yaml(to_raw);

    let mut changes = Vec::new();
    compare_values("", &from_val, &to_val, &mut changes);
    changes
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_yaml(raw: Option<&str>) -> Value {
    match raw {
        None => Value::Null,
        Some(s) => {
            // serde_yaml → serde_json::Value via intermediate YAML value.
            // This lets us use serde_json pointers / ordering for comparison.
            match serde_yaml::from_str::<serde_yaml::Value>(s) {
                Ok(y) => yaml_to_json(y),
                Err(_) => Value::Null,
            }
        }
    }
}

/// Convert a `serde_yaml::Value` into a `serde_json::Value` for uniform comparison.
fn yaml_to_json(y: serde_yaml::Value) -> Value {
    // Round-trip through JSON serialisation - simple and correct.
    let json_str = serde_json::to_string(&y).unwrap_or_default();
    serde_json::from_str(&json_str).unwrap_or(Value::Null)
}

/// Recursively walk two JSON values, emitting a `Change` for every leaf that differs.
fn compare_values(path: &str, from: &Value, to: &Value, out: &mut Vec<Change>) {
    match (from, to) {
        // Both null - no change.
        (Value::Null, Value::Null) => {}

        // From has a value, to does not → removed.
        (f, Value::Null) if !matches!(f, Value::Null) => {
            if let Value::Object(fm) = f {
                // Object → Null: each field is individually removed.
                compare_objects(path, fm, &serde_json::Map::new(), out);
            } else {
                out.push(make_change(path, ChangeKind::Removed, Some(f.clone()), None));
            }
        }

        // From is null, to has a value → added.
        (Value::Null, t) => {
            if let Value::Object(tm) = t {
                // Null → Object: each field is individually added.
                compare_objects(path, &serde_json::Map::new(), tm, out);
            } else {
                out.push(make_change(path, ChangeKind::Added, None, Some(t.clone())));
            }
        }

        // Both are objects - recurse per-key.
        (Value::Object(fm), Value::Object(tm)) => {
            compare_objects(path, fm, tm, out);
        }

        // Both are arrays - compare positionally; treat array as atomic value.
        (Value::Array(_), Value::Array(_)) if from == to => {}
        (Value::Array(_), Value::Array(_)) => {
            out.push(make_change(
                path,
                ChangeKind::Modified,
                Some(from.clone()),
                Some(to.clone()),
            ));
        }

        // Scalars - compare directly.
        (f, t) if f == t => {}
        (f, t) => {
            out.push(make_change(
                path,
                ChangeKind::Modified,
                Some(f.clone()),
                Some(t.clone()),
            ));
        }
    }
}

fn compare_objects(
    base_path: &str,
    from: &serde_json::Map<String, Value>,
    to: &serde_json::Map<String, Value>,
    out: &mut Vec<Change>,
) {
    // Collect all keys from both sides.
    let mut all_keys: Vec<&str> = Vec::new();
    for k in from.keys() {
        all_keys.push(k.as_str());
    }
    for k in to.keys() {
        if !from.contains_key(k.as_str()) {
            all_keys.push(k.as_str());
        }
    }
    // Sort for deterministic output.
    all_keys.sort_unstable();

    for key in all_keys {
        let child_path = if base_path.is_empty() {
            key.to_string()
        } else {
            format!("{base_path}.{key}")
        };
        let f = from.get(key).unwrap_or(&Value::Null);
        let t = to.get(key).unwrap_or(&Value::Null);
        compare_values(&child_path, f, t, out);
    }
}

fn make_change(
    path: &str,
    kind: ChangeKind,
    before: Option<Value>,
    after: Option<Value>,
) -> Change {
    let category = classify_path(path);
    Change {
        category,
        path: path.to_string(),
        kind,
        before,
        after,
        line_range: None,
    }
}

/// Map a frontmatter dotted-path to a [`ChangeCategory`].
fn classify_path(path: &str) -> ChangeCategory {
    let top = path.split('.').next().unwrap_or("");
    match top {
        "version" => ChangeCategory::Version,
        "model" => ChangeCategory::SemanticModel,
        "input" | "output" => ChangeCategory::SemanticSchema,
        "evals" => ChangeCategory::SemanticEvals,
        "chains_to" => ChangeCategory::SemanticChains,
        "description" | "id" => ChangeCategory::Metadata,
        "intentry" => {
            // Sub-keys like tags, license are metadata; parent/forked_at are also metadata.
            ChangeCategory::Metadata
        }
        _ => ChangeCategory::Metadata,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn changes(from: Option<&str>, to: Option<&str>) -> Vec<Change> {
        diff_frontmatter(from, to)
    }

    // -- No-change cases -----------------------------------------------------

    #[test]
    fn identical_frontmatter_no_changes() {
        let fm = "id: summarise\nversion: 1.0.0\ndescription: A prompt";
        assert!(changes(Some(fm), Some(fm)).is_empty());
    }

    #[test]
    fn both_none_no_changes() {
        assert!(changes(None, None).is_empty());
    }

    // -- Scalar changes -------------------------------------------------------

    #[test]
    fn description_changed_is_metadata() {
        let from = "id: x\nversion: 1.0.0\ndescription: old";
        let to   = "id: x\nversion: 1.0.0\ndescription: new";
        let cs = changes(Some(from), Some(to));
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].path, "description");
        assert_eq!(cs[0].kind, ChangeKind::Modified);
        assert_eq!(cs[0].category, ChangeCategory::Metadata);
    }

    #[test]
    fn version_change_is_version_category() {
        let from = "id: x\nversion: 1.0.0";
        let to   = "id: x\nversion: 1.1.0";
        let cs = changes(Some(from), Some(to));
        let v = cs.iter().find(|c| c.path == "version").expect("version change");
        assert_eq!(v.category, ChangeCategory::Version);
        assert_eq!(v.kind, ChangeKind::Modified);
    }

    #[test]
    fn model_temperature_changed_is_semantic_model() {
        let from = "model:\n  temperature: 0.5";
        let to   = "model:\n  temperature: 0.9";
        let cs = changes(Some(from), Some(to));
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].path, "model.temperature");
        assert_eq!(cs[0].category, ChangeCategory::SemanticModel);
    }

    #[test]
    fn model_added_is_semantic_model() {
        let from = "id: x\nversion: 1.0.0";
        let to   = "id: x\nversion: 1.0.0\nmodel:\n  temperature: 0.7";
        let cs = changes(Some(from), Some(to));
        let m = cs.iter().find(|c| c.path.starts_with("model")).expect("model change");
        assert_eq!(m.category, ChangeCategory::SemanticModel);
        assert_eq!(m.kind, ChangeKind::Added);
    }

    #[test]
    fn model_removed_is_semantic_model() {
        let from = "id: x\nversion: 1.0.0\nmodel:\n  temperature: 0.7";
        let to   = "id: x\nversion: 1.0.0";
        let cs = changes(Some(from), Some(to));
        let m = cs.iter().find(|c| c.path.starts_with("model")).expect("model change");
        assert_eq!(m.category, ChangeCategory::SemanticModel);
        assert_eq!(m.kind, ChangeKind::Removed);
    }

    // -- Schema changes -------------------------------------------------------

    #[test]
    fn input_schema_added_is_semantic_schema() {
        let from = "id: x\nversion: 1.0.0";
        let to   = "id: x\nversion: 1.0.0\ninput:\n  schema:\n    name: string";
        let cs = changes(Some(from), Some(to));
        let s = cs.iter().find(|c| c.path.starts_with("input")).expect("input change");
        assert_eq!(s.category, ChangeCategory::SemanticSchema);
    }

    #[test]
    fn output_schema_removed_is_semantic_schema() {
        let from = "id: x\nversion: 1.0.0\noutput:\n  schema:\n    result: string";
        let to   = "id: x\nversion: 1.0.0";
        let cs = changes(Some(from), Some(to));
        let s = cs.iter().find(|c| c.path.starts_with("output")).expect("output change");
        assert_eq!(s.category, ChangeCategory::SemanticSchema);
        assert_eq!(s.kind, ChangeKind::Removed);
    }

    // -- Evals ---------------------------------------------------------------

    #[test]
    fn evals_changed_is_semantic_evals() {
        let from = "id: x\nversion: 1.0.0\nevals:\n  - input: {a: 1}";
        let to   = "id: x\nversion: 1.0.0\nevals:\n  - input: {a: 2}";
        let cs = changes(Some(from), Some(to));
        let e = cs.iter().find(|c| c.path.starts_with("evals")).expect("evals change");
        assert_eq!(e.category, ChangeCategory::SemanticEvals);
    }

    // -- Tier transitions -----------------------------------------------------

    #[test]
    fn tier1_to_tier2_adds_id_version_is_metadata_version() {
        // from: no frontmatter (Tier 1), to: has id + version (Tier 2)
        let to = "id: my-prompt\nversion: 1.0.0";
        let cs = changes(None, Some(to));
        let has_version = cs.iter().any(|c| c.path == "version" && c.kind == ChangeKind::Added);
        let has_id      = cs.iter().any(|c| c.path == "id"      && c.kind == ChangeKind::Added);
        assert!(has_version, "expected version Added change");
        assert!(has_id,      "expected id Added change");
    }

    // -- Chains_to -----------------------------------------------------------

    #[test]
    fn chains_to_added_is_semantic_chains() {
        let from = "id: x\nversion: 1.0.0";
        let to   = "id: x\nversion: 1.0.0\nchains_to:\n  - step2";
        let cs = changes(Some(from), Some(to));
        let c = cs.iter().find(|c| c.path.starts_with("chains_to")).expect("chains_to change");
        assert_eq!(c.category, ChangeCategory::SemanticChains);
    }

    // -- Intentry metadata ---------------------------------------------------

    #[test]
    fn intentry_tags_changed_is_metadata() {
        let from = "id: x\nversion: 1.0.0\nintentry:\n  tags: [a, b]";
        let to   = "id: x\nversion: 1.0.0\nintentry:\n  tags: [a, b, c]";
        let cs = changes(Some(from), Some(to));
        let m = cs.iter().find(|c| c.path.starts_with("intentry")).expect("intentry change");
        assert_eq!(m.category, ChangeCategory::Metadata);
    }

    // -- Multiple concurrent changes -----------------------------------------

    #[test]
    fn multiple_fields_changed() {
        let from = "id: x\nversion: 1.0.0\ndescription: old\nmodel:\n  temperature: 0.3";
        let to   = "id: x\nversion: 1.1.0\ndescription: new\nmodel:\n  temperature: 0.7";
        let cs = changes(Some(from), Some(to));
        assert!(cs.len() >= 3, "expected ≥3 changes, got {}", cs.len());
        let paths: Vec<_> = cs.iter().map(|c| c.path.as_str()).collect();
        assert!(paths.contains(&"version"));
        assert!(paths.contains(&"description"));
        assert!(paths.contains(&"model.temperature"));
    }

    // -- Edge cases ----------------------------------------------------------

    #[test]
    fn malformed_yaml_treated_as_null() {
        // Non-YAML garbage → parsed as Null → whole thing is "removed" vs "added"
        let cs = changes(Some(": this is not valid yaml: ["), Some("id: x"));
        // Should not panic; should produce some changes
        let _ = cs;
    }

    #[test]
    fn empty_string_frontmatter_is_null() {
        let cs = changes(Some(""), Some("id: x\nversion: 1.0.0"));
        assert!(!cs.is_empty());
    }
}
