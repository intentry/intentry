use intr_parser::Eval;
use serde_json::Value;

use super::types::{Change, ChangeCategory, ChangeKind};

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Compare two lists of eval cases.
///
/// Evals are matched by their `description` field when present; otherwise by
/// their `input` value (serialised to JSON). Unmatched evals on the "from"
/// side are Removed; unmatched on the "to" side are Added.
pub fn diff_evals(from: Option<&[Eval]>, to: Option<&[Eval]>) -> Vec<Change> {
    let from_list = from.unwrap_or(&[]);
    let to_list   = to.unwrap_or(&[]);

    if from_list.is_empty() && to_list.is_empty() {
        return vec![];
    }

    let mut changes = Vec::new();

    // Build a key for matching evals: prefer description, fall back to input JSON.
    let key_of = |e: &Eval| -> String {
        e.description
            .clone()
            .unwrap_or_else(|| serde_json::to_string(&e.input).unwrap_or_default())
    };

    // Index "to" evals by key for O(n) matching.
    let mut to_map: std::collections::HashMap<String, &Eval> = to_list
        .iter()
        .map(|e| (key_of(e), e))
        .collect();

    for from_eval in from_list {
        let k = key_of(from_eval);
        if let Some(to_eval) = to_map.remove(&k) {
            // Matched - check if content changed.
            let from_val = eval_to_value(from_eval);
            let to_val   = eval_to_value(to_eval);
            if from_val != to_val {
                changes.push(Change {
                    category: ChangeCategory::SemanticEvals,
                    path: format!("evals[{k}]"),
                    kind: ChangeKind::Modified,
                    before: Some(from_val),
                    after:  Some(to_val),
                    line_range: None,
                });
            }
        } else {
            // Not in "to" → removed.
            changes.push(Change {
                category: ChangeCategory::SemanticEvals,
                path: format!("evals[{k}]"),
                kind: ChangeKind::Removed,
                before: Some(eval_to_value(from_eval)),
                after:  None,
                line_range: None,
            });
        }
    }

    // Anything remaining in to_map was not in "from" → added.
    for (k, to_eval) in to_map {
        changes.push(Change {
            category: ChangeCategory::SemanticEvals,
            path: format!("evals[{k}]"),
            kind: ChangeKind::Added,
            before: None,
            after:  Some(eval_to_value(to_eval)),
            line_range: None,
        });
    }

    changes
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn eval_to_value(e: &Eval) -> Value {
    serde_json::to_value(e).unwrap_or(Value::Null)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use intr_parser::Eval;

    fn eval(description: &str, input: serde_json::Value) -> Eval {
        Eval {
            description: Some(description.to_string()),
            input,
            expect: None,
        }
    }

    #[test]
    fn no_evals_no_changes() {
        assert!(diff_evals(None, None).is_empty());
    }

    #[test]
    fn identical_evals_no_changes() {
        let e = eval("basic", serde_json::json!({"text": "hello"}));
        assert!(diff_evals(Some(&[e.clone()]), Some(&[e])).is_empty());
    }

    #[test]
    fn eval_added() {
        let new_eval = eval("extra", serde_json::json!({"text": "world"}));
        let cs = diff_evals(Some(&[]), Some(&[new_eval]));
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].kind, ChangeKind::Added);
        assert_eq!(cs[0].category, ChangeCategory::SemanticEvals);
    }

    #[test]
    fn eval_removed() {
        let e = eval("gone", serde_json::json!({"text": "bye"}));
        let cs = diff_evals(Some(&[e]), Some(&[]));
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].kind, ChangeKind::Removed);
    }

    #[test]
    fn eval_modified_same_description() {
        let from = eval("test-1", serde_json::json!({"text": "old"}));
        let to   = eval("test-1", serde_json::json!({"text": "new"}));
        let cs = diff_evals(Some(&[from]), Some(&[to]));
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].kind, ChangeKind::Modified);
        assert_eq!(cs[0].path, "evals[test-1]");
    }

    #[test]
    fn multiple_evals_partial_change() {
        let from = vec![
            eval("alpha", serde_json::json!({"x": 1})),
            eval("beta",  serde_json::json!({"x": 2})),
        ];
        let to = vec![
            eval("alpha", serde_json::json!({"x": 1})),  // unchanged
            eval("beta",  serde_json::json!({"x": 99})), // modified
            eval("gamma", serde_json::json!({"x": 3})),  // added
        ];
        let cs = diff_evals(Some(&from), Some(&to));
        assert_eq!(cs.len(), 2, "expected beta-modified + gamma-added");
        let modified = cs.iter().find(|c| c.kind == ChangeKind::Modified).expect("modified");
        assert_eq!(modified.path, "evals[beta]");
        let added = cs.iter().find(|c| c.kind == ChangeKind::Added).expect("added");
        assert_eq!(added.path, "evals[gamma]");
    }

    #[test]
    fn eval_without_description_matched_by_input() {
        let from_eval = Eval {
            description: None,
            input: serde_json::json!({"key": "value"}),
            expect: None,
        };
        let to_eval = Eval {
            description: None,
            input: serde_json::json!({"key": "value"}),
            expect: None,
        };
        assert!(diff_evals(Some(&[from_eval]), Some(&[to_eval])).is_empty());
    }

    #[test]
    fn none_from_all_evals_added() {
        let evals = vec![
            eval("a", serde_json::json!({})),
            eval("b", serde_json::json!({})),
        ];
        let cs = diff_evals(None, Some(&evals));
        assert_eq!(cs.len(), 2);
        assert!(cs.iter().all(|c| c.kind == ChangeKind::Added));
    }

    #[test]
    fn none_to_all_evals_removed() {
        let evals = vec![eval("a", serde_json::json!({}))];
        let cs = diff_evals(Some(&evals), None);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].kind, ChangeKind::Removed);
    }
}
