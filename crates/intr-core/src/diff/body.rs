use similar::{ChangeTag, TextDiff};

use super::types::{Change, ChangeCategory, ChangeKind, LineRange};

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Diff two template body strings using Myers line-based diff.
///
/// Returns one [`Change`] per contiguous hunk of added or removed lines.
/// Purely cosmetic changes (whitespace-only lines) are classified as
/// [`ChangeCategory::Cosmetic`]; substantive body changes are
/// [`ChangeCategory::SemanticTemplate`].
pub fn diff_body(from: &str, to: &str) -> Vec<Change> {
    if from == to {
        return vec![];
    }

    let diff = TextDiff::from_lines(from, to);
    let mut changes = Vec::new();

    // Walk the unified changeset and collect contiguous hunks.
    let mut hunk_lines_removed: Vec<(u32, &str)> = Vec::new(); // (line_no_in_from, text)
    let mut hunk_lines_added: Vec<(u32, &str)> = Vec::new();   // (line_no_in_to, text)

    let ops = diff.ops();
    for op in ops {
        for change in diff.iter_changes(op) {
            match change.tag() {
                ChangeTag::Equal => {
                    flush_hunk(&mut hunk_lines_removed, &mut hunk_lines_added, &mut changes);
                }
                ChangeTag::Delete => {
                    let lineno = change.old_index().map(|i| i as u32 + 1).unwrap_or(0);
                    hunk_lines_removed.push((lineno, change.value()));
                }
                ChangeTag::Insert => {
                    let lineno = change.new_index().map(|i| i as u32 + 1).unwrap_or(0);
                    hunk_lines_added.push((lineno, change.value()));
                }
            }
        }
    }
    // Flush any remaining hunk at end-of-file.
    flush_hunk(&mut hunk_lines_removed, &mut hunk_lines_added, &mut changes);

    changes
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Flush accumulated removed + added lines into one or two [`Change`] entries.
fn flush_hunk(
    removed: &mut Vec<(u32, &str)>,
    added: &mut Vec<(u32, &str)>,
    out: &mut Vec<Change>,
) {
    if removed.is_empty() && added.is_empty() {
        return;
    }

    if !removed.is_empty() && !added.is_empty() {
        // Lines replaced - emit as a single "Modified" change covering both ranges.
        let from_start = removed[0].0;
        let from_end   = removed.last().unwrap().0;
        let to_start   = added[0].0;
        let to_end     = added.last().unwrap().0;

        let before_text: String = removed.iter().map(|(_, l)| *l).collect();
        let after_text:  String = added.iter().map(|(_, l)| *l).collect();

        let category = classify_body_change(&before_text, &after_text);
        out.push(Change {
            category,
            path: format!("body[{from_start}:{from_end}→{to_start}:{to_end}]"),
            kind: ChangeKind::Modified,
            before: Some(serde_json::Value::String(before_text)),
            after:  Some(serde_json::Value::String(after_text)),
            line_range: Some(LineRange { start: to_start, end: to_end }),
        });
    } else if !removed.is_empty() {
        let start = removed[0].0;
        let end   = removed.last().unwrap().0;
        let text: String = removed.iter().map(|(_, l)| *l).collect();
        let category = classify_body_change(&text, "");
        out.push(Change {
            category,
            path: format!("body[{start}:{end}]"),
            kind: ChangeKind::Removed,
            before: Some(serde_json::Value::String(text)),
            after:  None,
            line_range: Some(LineRange { start, end }),
        });
    } else {
        let start = added[0].0;
        let end   = added.last().unwrap().0;
        let text: String = added.iter().map(|(_, l)| *l).collect();
        let category = classify_body_change("", &text);
        out.push(Change {
            category,
            path: format!("body[{start}:{end}]"),
            kind: ChangeKind::Added,
            before: None,
            after:  Some(serde_json::Value::String(text)),
            line_range: Some(LineRange { start, end }),
        });
    }

    removed.clear();
    added.clear();
}

/// Decide whether a body hunk is cosmetic (whitespace/blank lines only) or semantic.
fn classify_body_change(before: &str, after: &str) -> ChangeCategory {
    let all_text = format!("{before}{after}");
    if all_text.chars().all(|c| c.is_whitespace()) {
        ChangeCategory::Cosmetic
    } else {
        ChangeCategory::SemanticTemplate
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_bodies_no_changes() {
        let body = "Hello, {{name}}!\nToday is {{day}}.";
        assert!(diff_body(body, body).is_empty());
    }

    #[test]
    fn empty_bodies_no_changes() {
        assert!(diff_body("", "").is_empty());
    }

    #[test]
    fn line_added_at_end() {
        let from = "Hello, {{name}}!\n";
        let to   = "Hello, {{name}}!\nExtra line.\n";
        let cs = diff_body(from, to);
        assert!(!cs.is_empty());
        let c = &cs[0];
        assert_eq!(c.kind, ChangeKind::Added);
        assert_eq!(c.category, ChangeCategory::SemanticTemplate);
        assert!(c.line_range.is_some());
    }

    #[test]
    fn line_removed() {
        let from = "Line one.\nLine two.\n";
        let to   = "Line one.\n";
        let cs = diff_body(from, to);
        assert!(!cs.is_empty());
        assert!(cs.iter().any(|c| c.kind == ChangeKind::Removed));
    }

    #[test]
    fn line_modified_is_semantic_template() {
        let from = "Summarise: {{text}}\n";
        let to   = "Translate: {{text}}\n";
        let cs = diff_body(from, to);
        assert!(!cs.is_empty());
        let c = &cs[0];
        assert_eq!(c.category, ChangeCategory::SemanticTemplate);
        assert_eq!(c.kind, ChangeKind::Modified);
    }

    #[test]
    fn whitespace_only_change_is_cosmetic() {
        let from = "Hello.\n";
        let to   = "Hello.\n\n";  // trailing blank line added
        let cs = diff_body(from, to);
        // The blank line is whitespace-only → cosmetic.
        assert!(cs.iter().any(|c| c.category == ChangeCategory::Cosmetic));
    }

    #[test]
    fn multi_hunk_change() {
        let from = "Line 1\nLine 2\nLine 3\nLine 4\n";
        let to   = "Line 1\nChanged 2\nLine 3\nChanged 4\n";
        let cs = diff_body(from, to);
        // Two non-adjacent modified hunks.
        assert!(cs.len() >= 2, "expected ≥2 hunks, got {}", cs.len());
    }

    #[test]
    fn line_range_populated_for_body_changes() {
        let from = "a\n";
        let to   = "b\n";
        let cs = diff_body(from, to);
        assert!(!cs.is_empty());
        assert!(cs[0].line_range.is_some());
    }

    #[test]
    fn complete_replacement() {
        let from = "Old content entirely.\n";
        let to   = "New content entirely.\n";
        let cs = diff_body(from, to);
        assert!(!cs.is_empty());
        assert_eq!(cs[0].kind, ChangeKind::Modified);
    }

    #[test]
    fn empty_to_populated() {
        let cs = diff_body("", "Some template text.\n");
        assert!(!cs.is_empty());
        assert_eq!(cs[0].kind, ChangeKind::Added);
    }

    #[test]
    fn populated_to_empty() {
        let cs = diff_body("Some template text.\n", "");
        assert!(!cs.is_empty());
        assert_eq!(cs[0].kind, ChangeKind::Removed);
    }
}
