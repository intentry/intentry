//! `intr_core::diff` - Semantic diff engine for `.prompt` files.
//!
//! This module compares two versions of an Intentry prompt file and produces a
//! structured [`DiffResult`] describing every change, along with a [`DiffSummary`]
//! that recommends the minimum semver bump required.
//!
//! ## Quick start
//!
//! ```rust,ignore
//! use intr_core::diff::{diff_content, diff_commits};
//!
//! // Compare raw content strings (no commit metadata):
//! let result = diff_content(old_content, new_content)?;
//!
//! // Compare two Commit records (adds version metadata to the result):
//! let result = diff_commits(&commit_from, &commit_to, old_content, new_content)?;
//!
//! // Format as unified text:
//! let text = intr_core::diff::format::text::render(&result);
//! ```
//!
//! ## Algorithm (5 steps)
//!
//! 1. **Parse** both sides with `intr-parser`; fall back to plain-text on failure.
//! 2. **Frontmatter diff** - deep YAML comparison with path tracking.
//! 3. **Body diff** - line-level Myers diff via the `similar` crate.
//! 4. **Eval diff** - match by description/input, detect Added/Removed/Modified.
//! 5. **Classify** - all changes → [`DiffSummary`] with semver-bump heuristics.

pub mod body;
pub mod classify;
pub mod error;
pub mod evals;
pub mod format;
pub mod frontmatter;
pub mod types;

pub use error::DiffError;
pub use types::{
    Change, ChangeCategory, ChangeKind, CommitRef, DiffResult, DiffSummary, LineRange, OutputDiff,
    RunResult,
};

use crate::types::Commit;
use intr_parser::parse;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Compare two raw `.prompt` content strings.
///
/// Parse failures are non-fatal - the engine falls back to a plain-text body
/// diff with no frontmatter changes. This matches the spec requirement that
/// diff should work even on Tier-1 (no frontmatter) files.
pub fn diff_content(from_content: &str, to_content: &str) -> Result<DiffResult, DiffError> {
    let from_parsed = parse(from_content.as_bytes()).ok();
    let to_parsed   = parse(to_content.as_bytes()).ok();

    // -- Extract frontmatter YAML strings ------------------------------------
    // We re-extract from the raw content rather than re-serialising from the
    // parsed struct to preserve exact YAML representation.
    let from_fm_raw = extract_frontmatter_raw(from_content);
    let to_fm_raw   = extract_frontmatter_raw(to_content);

    // -- Extract body -------------------------------------------------------
    let from_body = from_parsed
        .as_ref()
        .map(|p| p.body.as_str())
        .unwrap_or(from_content);
    let to_body = to_parsed
        .as_ref()
        .map(|p| p.body.as_str())
        .unwrap_or(to_content);

    // -- Extract evals from parsed frontmatter ------------------------------
    let from_evals = from_parsed
        .as_ref()
        .and_then(|p| p.frontmatter.as_ref())
        .and_then(|fm| fm.evals.as_deref());
    let to_evals = to_parsed
        .as_ref()
        .and_then(|p| p.frontmatter.as_ref())
        .and_then(|fm| fm.evals.as_deref());

    // -- Run all diff passes ------------------------------------------------
    let mut all_changes = Vec::new();

    // 1. Frontmatter diff
    let fm_changes = frontmatter::diff_frontmatter(from_fm_raw, to_fm_raw);
    all_changes.extend(fm_changes);

    // 2. Body diff (evals are in the frontmatter, not the body)
    let body_changes = body::diff_body(from_body, to_body);
    all_changes.extend(body_changes);

    // 3. Eval diff
    let eval_changes = evals::diff_evals(from_evals, to_evals);
    // Deduplicate: frontmatter diff already captures the `evals` key as a raw
    // JSON blob change; the eval-specific pass gives richer per-case changes.
    // Remove the coarse `evals` change from the frontmatter pass and keep the
    // fine-grained ones from the eval pass.
    all_changes.retain(|c| !matches!(c.category, ChangeCategory::SemanticEvals));
    all_changes.extend(eval_changes);

    // 4. Classify → summary
    let summary = classify::classify(&all_changes);

    Ok(DiffResult {
        from: None,
        to: None,
        changes: all_changes,
        summary,
    })
}

/// Compare two [`Commit`] records, adding [`CommitRef`] metadata to the result.
///
/// `content_from` and `content_to` must be the raw `.prompt` file bytes for
/// each commit. These are typically fetched from blob storage before calling
/// this function.
pub fn diff_commits(
    from: &Commit,
    to: &Commit,
    content_from: &str,
    content_to: &str,
) -> Result<DiffResult, DiffError> {
    let mut result = diff_content(content_from, content_to)?;

    result.from = Some(CommitRef {
        commit_id: from.id.clone(),
        version: from.version.clone(),
    });
    result.to = Some(CommitRef {
        commit_id: to.id.clone(),
        version: to.version.clone(),
    });

    Ok(result)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Extract the raw YAML content between `---` fences, if present.
/// Returns `None` if no frontmatter block is found.
fn extract_frontmatter_raw(content: &str) -> Option<&str> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return None;
    }
    // Skip the opening `---` line.
    let rest = content.trim_start_matches("---").trim_start_matches('\n');
    // Find the closing `---`.
    if let Some(end) = rest.find("\n---") {
        Some(&rest[..end])
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Integration tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const PROMPT_V1: &str = r#"---
id: summarise
version: 1.0.0
description: Summarise a document.
model:
  temperature: 0.5
input:
  schema:
    text: string
---
Summarise the following: {{text}}
"#;

    const PROMPT_V2: &str = r#"---
id: summarise
version: 1.1.0
description: Summarise a document concisely.
model:
  temperature: 0.7
input:
  schema:
    text: string
---
Provide a concise summary of: {{text}}
"#;

    const PROMPT_TIER1: &str = "Hello, {{name}}! Today is {{day}}.";

    const PROMPT_SCHEMA_REMOVED: &str = r#"---
id: summarise
version: 2.0.0
description: No schema now.
---
Summarise: {{text}}
"#;

    // -- Basic integration --------------------------------------------------

    #[test]
    fn identical_prompts_no_changes() {
        let r = diff_content(PROMPT_V1, PROMPT_V1).unwrap();
        assert!(r.changes.is_empty(), "expected no changes, got {:?}", r.changes);
    }

    #[test]
    fn v1_to_v2_detects_multiple_changes() {
        let r = diff_content(PROMPT_V1, PROMPT_V2).unwrap();
        assert!(!r.changes.is_empty());
        // Should detect: version bump, description change, temperature change, body change.
        let paths: Vec<_> = r.changes.iter().map(|c| c.path.as_str()).collect();
        assert!(paths.iter().any(|p| *p == "version"), "expected version change");
        assert!(paths.iter().any(|p| *p == "model.temperature"), "expected model.temperature change");
    }

    #[test]
    fn schema_removal_suggests_major() {
        let r = diff_content(PROMPT_V1, PROMPT_SCHEMA_REMOVED).unwrap();
        assert_eq!(r.summary.suggested_version_bump, crate::version::BumpKind::Major);
        assert!(r.summary.is_breaking);
    }

    // -- Tier 1 (no frontmatter) -------------------------------------------

    #[test]
    fn tier1_identical_no_changes() {
        let r = diff_content(PROMPT_TIER1, PROMPT_TIER1).unwrap();
        assert!(r.changes.is_empty());
    }

    #[test]
    fn tier1_body_change_is_semantic_template() {
        let from = "Hello, {{name}}!";
        let to   = "Goodbye, {{name}}!";
        let r = diff_content(from, to).unwrap();
        assert!(r.changes.iter().any(|c| matches!(c.category, ChangeCategory::SemanticTemplate)));
    }

    // -- Eval diff integration ----------------------------------------------

    #[test]
    fn eval_added_detected() {
        let from = r#"---
id: x
version: 1.0.0
---
Template {{var}}
"#;
        let to = r#"---
id: x
version: 1.0.0
evals:
  - description: basic
    input:
      var: hello
---
Template {{var}}
"#;
        let r = diff_content(from, to).unwrap();
        let eval_change = r
            .changes
            .iter()
            .find(|c| matches!(c.category, ChangeCategory::SemanticEvals));
        assert!(eval_change.is_some(), "expected an evals change");
        assert_eq!(eval_change.unwrap().kind, ChangeKind::Added);
    }

    // -- Frontmatter raw extraction ----------------------------------------

    #[test]
    fn extract_frontmatter_raw_present() {
        let fm = extract_frontmatter_raw(PROMPT_V1);
        assert!(fm.is_some());
        let s = fm.unwrap();
        assert!(s.contains("id: summarise"));
    }

    #[test]
    fn extract_frontmatter_raw_tier1() {
        assert!(extract_frontmatter_raw(PROMPT_TIER1).is_none());
    }

    // -- Summary correctness ------------------------------------------------

    #[test]
    fn summary_counts_match_changes() {
        let r = diff_content(PROMPT_V1, PROMPT_V2).unwrap();
        let manual_semantic = r
            .changes
            .iter()
            .filter(|c| {
                !matches!(
                    c.category,
                    ChangeCategory::Cosmetic | ChangeCategory::Metadata | ChangeCategory::Version
                )
            })
            .count() as u32;
        let manual_cosmetic = r
            .changes
            .iter()
            .filter(|c| {
                matches!(
                    c.category,
                    ChangeCategory::Cosmetic | ChangeCategory::Metadata | ChangeCategory::Version
                )
            })
            .count() as u32;
        assert_eq!(r.summary.semantic_changes, manual_semantic);
        assert_eq!(r.summary.cosmetic_changes, manual_cosmetic);
    }

    // -- Formatters smoke tests (don't panic) -------------------------------

    #[test]
    fn text_formatter_does_not_panic() {
        let r = diff_content(PROMPT_V1, PROMPT_V2).unwrap();
        let text = format::text::render(&r);
        assert!(!text.is_empty());
        assert!(text.contains("diff --intentry"));
    }

    #[test]
    fn html_formatter_does_not_panic() {
        let r = diff_content(PROMPT_V1, PROMPT_V2).unwrap();
        let html = format::html::render(&r);
        assert!(!html.is_empty());
        assert!(html.contains("<div class=\"intr-diff\">"));
    }
}
