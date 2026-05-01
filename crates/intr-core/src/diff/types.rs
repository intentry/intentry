use crate::{ids::CommitId, version::BumpKind};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// CommitRef
// ---------------------------------------------------------------------------

/// A lightweight reference to a commit - used in [`super::DiffResult`] to
/// identify which two commits are being compared.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommitRef {
    pub commit_id: CommitId,
    pub version: semver::Version,
}

// ---------------------------------------------------------------------------
// LineRange
// ---------------------------------------------------------------------------

/// Inclusive 1-based line range within the template body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

// ---------------------------------------------------------------------------
// ChangeCategory
// ---------------------------------------------------------------------------

/// Classifies *what* changed so callers can filter by concern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeCategory {
    /// A change to the Handlebars template body that affects semantics.
    SemanticTemplate,
    /// A change to the `model:` frontmatter block.
    SemanticModel,
    /// A change to `input.schema` or `output.schema`.
    SemanticSchema,
    /// A change to the `evals:` list.
    SemanticEvals,
    /// A change to `chains_to:` references.
    SemanticChains,
    /// A change to human-readable metadata: `description`, `tags`, etc.
    Metadata,
    /// A whitespace-only or comment-only change with no semantic impact.
    Cosmetic,
    /// A change to the `version:` field in frontmatter.
    Version,
}

// ---------------------------------------------------------------------------
// ChangeKind
// ---------------------------------------------------------------------------

/// Whether a field was added, removed, or modified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
}

// ---------------------------------------------------------------------------
// Change
// ---------------------------------------------------------------------------

/// A single logical change between two commits.
///
/// `before` and `after` are `None` when a field is entirely absent on that
/// side (i.e. `ChangeKind::Added` means `before == None`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    /// Broad classification of what this change affects.
    pub category: ChangeCategory,
    /// Dotted-path string describing the field that changed (e.g. `"model.temperature"`).
    pub path: String,
    /// Whether the field was added, removed, or modified.
    pub kind: ChangeKind,
    /// Value on the "from" side. `None` when `kind == Added`.
    pub before: Option<serde_json::Value>,
    /// Value on the "to" side. `None` when `kind == Removed`.
    pub after: Option<serde_json::Value>,
    /// Line range in the template body - populated only for `SemanticTemplate` / `Cosmetic` changes.
    pub line_range: Option<LineRange>,
}

// ---------------------------------------------------------------------------
// DiffSummary
// ---------------------------------------------------------------------------

/// Aggregate statistics computed from all [`Change`] items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffSummary {
    /// Number of changes classified as semantic.
    pub semantic_changes: u32,
    /// Number of changes classified as cosmetic.
    pub cosmetic_changes: u32,
    /// `true` if any change implies a breaking public API change (schema removal, etc.).
    pub is_breaking: bool,
    /// The minimum semver bump required to publish these changes.
    pub suggested_version_bump: BumpKind,
}

// ---------------------------------------------------------------------------
// DiffResult
// ---------------------------------------------------------------------------

/// The full result of comparing two commits or two raw content strings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffResult {
    /// The "from" commit reference. `None` when using [`super::diff_content`] directly.
    pub from: Option<CommitRef>,
    /// The "to" commit reference. `None` when using [`super::diff_content`] directly.
    pub to: Option<CommitRef>,
    /// All individual changes detected.
    pub changes: Vec<Change>,
    /// Aggregate summary with semver guidance.
    pub summary: DiffSummary,
}

// ---------------------------------------------------------------------------
// RunResult / OutputDiff (V1 stub - output similarity gated for V1.5)
// ---------------------------------------------------------------------------

/// The result of executing a prompt against a model during an output diff.
/// Kept as a minimal struct for V1; richer fields added in V1.5.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunResult {
    pub output_text: String,
    pub model_id: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// A comparison between running two versions of a prompt against the same input.
///
/// In V1, `output_similarity` is always `0.0` and `semantic_match` is always
/// `None` - the embedding-based comparison is deferred to V1.5.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputDiff {
    pub input: serde_json::Value,
    pub left: RunResult,
    pub right: RunResult,
    /// Cosine similarity of embeddings - V1 stub, always 0.0.
    pub output_similarity: f32,
    /// Whether outputs are semantically equivalent - V1 stub, always `None`.
    pub semantic_match: Option<bool>,
    /// Token-level structural diff of the two versions.
    pub token_diff: DiffResult,
}
