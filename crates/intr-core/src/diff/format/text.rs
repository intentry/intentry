use std::fmt::Write;

use crate::diff::types::{Change, ChangeKind, DiffResult};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Render a [`DiffResult`] as a unified-style plain-text diff string.
///
/// Format:
/// ```text
/// diff --intentry from=1.0.0 to=1.1.0
/// --- a/<commit_id_short>
/// +++ b/<commit_id_short>
///
/// [Category: SemanticTemplate]
/// @@ body[5:7→5:8] @@
/// -old line
/// +new line
///
/// [Category: SemanticModel]
/// @@ model.temperature @@
/// -0.5
/// +0.9
///
/// Summary: 2 semantic, 0 cosmetic change(s) — suggested bump: patch
/// ```
pub fn render(result: &DiffResult) -> String {
    let mut out = String::new();

    // Header
    let from_ver = result
        .from
        .as_ref()
        .map(|r| r.version.to_string())
        .unwrap_or_else(|| "?".to_string());
    let to_ver = result
        .to
        .as_ref()
        .map(|r| r.version.to_string())
        .unwrap_or_else(|| "?".to_string());
    let from_id = result
        .from
        .as_ref()
        .map(|r| short_id(r.commit_id.to_string()))
        .unwrap_or_else(|| "from".to_string());
    let to_id = result
        .to
        .as_ref()
        .map(|r| short_id(r.commit_id.to_string()))
        .unwrap_or_else(|| "to".to_string());

    writeln!(out, "diff --intentry from={from_ver} to={to_ver}").unwrap();
    writeln!(out, "--- a/{from_id}").unwrap();
    writeln!(out, "+++ b/{to_id}").unwrap();

    if result.changes.is_empty() {
        writeln!(out, "\n(no changes)").unwrap();
    } else {
        for change in &result.changes {
            render_change(&mut out, change);
        }
    }

    // Footer summary
    writeln!(out).unwrap();
    writeln!(
        out,
        "Summary: {} semantic, {} cosmetic change(s) — suggested bump: {}",
        result.summary.semantic_changes,
        result.summary.cosmetic_changes,
        result.summary.suggested_version_bump,
    )
    .unwrap();

    if result.summary.is_breaking {
        writeln!(out, "⚠  Breaking change detected.").unwrap();
    }

    out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn render_change(out: &mut String, change: &Change) {
    writeln!(out).unwrap();
    writeln!(out, "[Category: {:?}]", change.category).unwrap();
    writeln!(out, "@@ {} @@", change.path).unwrap();

    match change.kind {
        ChangeKind::Modified => {
            if let Some(before) = &change.before {
                for line in value_lines(before) {
                    writeln!(out, "-{line}").unwrap();
                }
            }
            if let Some(after) = &change.after {
                for line in value_lines(after) {
                    writeln!(out, "+{line}").unwrap();
                }
            }
        }
        ChangeKind::Removed => {
            if let Some(before) = &change.before {
                for line in value_lines(before) {
                    writeln!(out, "-{line}").unwrap();
                }
            }
        }
        ChangeKind::Added => {
            if let Some(after) = &change.after {
                for line in value_lines(after) {
                    writeln!(out, "+{line}").unwrap();
                }
            }
        }
    }
}

/// Turn a JSON value into display lines (multi-line for strings, single line for others).
fn value_lines(v: &serde_json::Value) -> Vec<String> {
    match v {
        serde_json::Value::String(s) => s.lines().map(str::to_string).collect(),
        other => vec![serde_json::to_string(other).unwrap_or_default()],
    }
}

/// Return the first 8 characters of an ID string (for display).
fn short_id(id: String) -> String {
    id.chars().take(8).collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::types::{Change, ChangeCategory, ChangeKind, DiffResult};
    

    fn simple_result(changes: Vec<Change>) -> DiffResult {
        use crate::diff::classify::classify;
        let summary = classify(&changes);
        DiffResult {
            from: None,
            to: None,
            changes,
            summary,
        }
    }

    #[test]
    fn no_changes_renders_no_changes_line() {
        let result = simple_result(vec![]);
        let text = render(&result);
        assert!(text.contains("(no changes)"));
        assert!(text.contains("Summary:"));
    }

    #[test]
    fn modified_change_renders_minus_plus() {
        let cs = vec![Change {
            category: ChangeCategory::SemanticTemplate,
            path: "body[1:1]".to_string(),
            kind: ChangeKind::Modified,
            before: Some(serde_json::Value::String("old line\n".to_string())),
            after:  Some(serde_json::Value::String("new line\n".to_string())),
            line_range: None,
        }];
        let text = render(&simple_result(cs));
        assert!(text.contains("-old line"));
        assert!(text.contains("+new line"));
    }

    #[test]
    fn summary_line_present() {
        let result = simple_result(vec![]);
        let text = render(&result);
        assert!(text.contains("suggested bump:"));
    }

    #[test]
    fn breaking_warning_present_when_schema_removed() {
        
        let cs = vec![Change {
            category: ChangeCategory::SemanticSchema,
            path: "output.schema".to_string(),
            kind: ChangeKind::Removed,
            before: Some(serde_json::json!({"result": "string"})),
            after: None,
            line_range: None,
        }];
        let text = render(&simple_result(cs));
        assert!(text.contains("Breaking change"));
    }
}
