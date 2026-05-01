use crate::diff::types::{Change, ChangeKind, DiffResult};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Render a [`DiffResult`] as a self-contained HTML fragment.
///
/// All user-supplied content is escaped via [`quick_xml::escape::escape`] to
/// prevent XSS.  The resulting HTML is suitable for embedding inside a larger
/// page - it is NOT a complete `<html>` document.
///
/// Structure:
/// ```html
/// <div class="intr-diff">
///   <div class="intr-diff-header">…</div>
///   <div class="intr-diff-changes">
///     <div class="intr-diff-change intr-change-modified">…</div>
///     …
///   </div>
///   <div class="intr-diff-summary">…</div>
/// </div>
/// ```
pub fn render(result: &DiffResult) -> String {
    let mut out = String::from("<div class=\"intr-diff\">\n");

    // -- Header -------------------------------------------------------------
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

    out.push_str(&format!(
        "  <div class=\"intr-diff-header\">Comparing <span class=\"intr-ver\">{}</span> → <span class=\"intr-ver\">{}</span></div>\n",
        esc(&from_ver),
        esc(&to_ver),
    ));

    // -- Changes -----------------------------------------------------------
    out.push_str("  <div class=\"intr-diff-changes\">\n");
    if result.changes.is_empty() {
        out.push_str("    <div class=\"intr-diff-empty\">No changes.</div>\n");
    } else {
        for change in &result.changes {
            render_change_html(&mut out, change);
        }
    }
    out.push_str("  </div>\n");

    // -- Summary -----------------------------------------------------------
    let breaking_html = if result.summary.is_breaking {
        "    <span class=\"intr-breaking\"> ⚠ Breaking change.</span>"
    } else {
        ""
    };
    out.push_str(&format!(
        "  <div class=\"intr-diff-summary\">{} semantic, {} cosmetic change(s). Suggested bump: <strong>{}</strong>.{}\n  </div>\n",
        result.summary.semantic_changes,
        result.summary.cosmetic_changes,
        esc(&format!("{:?}", result.summary.suggested_version_bump).to_lowercase()),
        breaking_html,
    ));

    out.push_str("</div>\n");
    out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn render_change_html(out: &mut String, change: &Change) {
    let kind_class = match change.kind {
        ChangeKind::Added    => "intr-change-added",
        ChangeKind::Removed  => "intr-change-removed",
        ChangeKind::Modified => "intr-change-modified",
    };
    let category_name = format!("{:?}", change.category);

    out.push_str(&format!(
        "    <div class=\"intr-diff-change {kind_class}\">\n"
    ));
    out.push_str(&format!(
        "      <div class=\"intr-change-meta\"><span class=\"intr-category\">{}</span> <code>{}</code></div>\n",
        esc(&category_name),
        esc(&change.path),
    ));

    if let Some(before) = &change.before {
        let text = value_display(before);
        out.push_str(&format!(
            "      <pre class=\"intr-before\">-{}</pre>\n",
            esc(&text)
        ));
    }
    if let Some(after) = &change.after {
        let text = value_display(after);
        out.push_str(&format!(
            "      <pre class=\"intr-after\">+{}</pre>\n",
            esc(&text)
        ));
    }

    out.push_str("    </div>\n");
}

/// Escape a string for safe embedding in HTML attribute/text context.
fn esc(s: &str) -> String {
    // Use quick-xml's escape function for correctness.
    quick_xml::escape::escape(s).into_owned()
}

fn value_display(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
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
    fn renders_html_root_div() {
        let html = render(&simple_result(vec![]));
        assert!(html.contains("<div class=\"intr-diff\">"));
        assert!(html.contains("</div>"));
    }

    #[test]
    fn no_changes_shows_empty_message() {
        let html = render(&simple_result(vec![]));
        assert!(html.contains("No changes."));
    }

    #[test]
    fn xss_in_before_value_is_escaped() {
        let cs = vec![Change {
            category: ChangeCategory::SemanticTemplate,
            path: "body[1:1]".to_string(),
            kind: ChangeKind::Modified,
            before: Some(serde_json::Value::String(
                "<script>alert('xss')</script>".to_string(),
            )),
            after: Some(serde_json::Value::String("safe".to_string())),
            line_range: None,
        }];
        let html = render(&simple_result(cs));
        // Must not contain unescaped angle brackets.
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn breaking_flag_in_summary() {
        let cs = vec![Change {
            category: ChangeCategory::SemanticSchema,
            path: "output.schema".to_string(),
            kind: ChangeKind::Removed,
            before: Some(serde_json::json!({"result": "string"})),
            after: None,
            line_range: None,
        }];
        let html = render(&simple_result(cs));
        assert!(html.contains("intr-breaking"));
    }

    #[test]
    fn added_change_uses_correct_css_class() {
        let cs = vec![Change {
            category: ChangeCategory::SemanticModel,
            path: "model.temperature".to_string(),
            kind: ChangeKind::Added,
            before: None,
            after:  Some(serde_json::json!(0.7)),
            line_range: None,
        }];
        let html = render(&simple_result(cs));
        assert!(html.contains("intr-change-added"));
    }
}
