use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Parse result
// ---------------------------------------------------------------------------

/// Output of parsing a `.prompt` file.
///
/// The `tier` field indicates the structured richness of the file:
/// - `1` - plain template body only, no frontmatter.
/// - `2` - YAML frontmatter present with at least `id` + `version`.
/// - `3` - Tier 2 + `evals` and/or `chains_to`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParseResult {
    /// Detected tier (1, 2, or 3).
    pub tier: u8,

    /// Parsed YAML frontmatter. `None` for Tier 1 files.
    pub frontmatter: Option<Frontmatter>,

    /// The Handlebars template body (everything after the closing `---`).
    pub body: String,

    /// Variables extracted from `{{variable}}` markers in the body.
    /// De-duplicated, sorted alphabetically.
    pub variables: Vec<String>,

    /// Non-fatal issues encountered during parsing.
    pub warnings: Vec<ParseWarning>,
}

// ---------------------------------------------------------------------------
// Frontmatter
// ---------------------------------------------------------------------------

/// YAML frontmatter between `---` fences.
///
/// Fields follow the Dotprompt spec + Intentry extensions under the `intentry:` key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frontmatter {
    /// Stable kebab-case identifier, unique within a Space. Max 64 chars.
    /// Required for Tier 2+.
    pub id: Option<String>,

    /// Semver version string (e.g. `"1.2.0"`). Required for Tier 2+.
    pub version: Option<String>,

    /// One-line human description of what the prompt does.
    pub description: Option<String>,

    /// Model preferences and generation parameters.
    pub model: Option<ModelHints>,

    /// Input variable declarations.
    pub input: Option<InputSpec>,

    /// Output shape expectations (for evals and schema validation).
    pub output: Option<OutputSpec>,

    /// Eval cases. Presence of this field (non-empty) promotes to Tier 3.
    pub evals: Option<Vec<Eval>>,

    /// Intentry-specific extensions (tags, license, parent fork info, etc.).
    pub intentry: Option<IntrEntryMeta>,

    /// Any unknown fields are preserved as raw JSON for forward-compatibility.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Model hints
// ---------------------------------------------------------------------------

/// Model preferences for executing this prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelHints {
    /// Ordered list of preferred model IDs. First supported model is used.
    /// Example: `["claude-sonnet-4-6", "gpt-4o"]`
    pub preferred: Option<Vec<String>>,

    /// Sampling temperature (0.0–2.0).
    pub temperature: Option<f64>,

    /// Maximum tokens to generate.
    pub max_tokens: Option<u32>,

    /// Top-p nucleus sampling parameter.
    pub top_p: Option<f64>,

    /// Stop sequences.
    pub stop: Option<Vec<String>>,

    /// Any model-specific parameters not covered above are passed through.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Input / Output specs (Picoschema)
// ---------------------------------------------------------------------------

/// Input variable declarations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputSpec {
    /// Picoschema describing the expected input variables.
    pub schema: Option<Picoschema>,
}

/// Output shape expectations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputSpec {
    /// Picoschema describing the expected output structure.
    pub schema: Option<Picoschema>,

    /// Output format hint: `"text"`, `"json"`, `"markdown"`.
    pub format: Option<String>,
}

/// Picoschema type.
///
/// Picoschema is the lightweight schema format from Dotprompt. In Intentry it is
/// represented as raw JSON (parsed from YAML) to keep the parser simple and to
/// remain forward-compatible as the schema language evolves.
///
/// Common examples:
/// ```yaml
/// schema:
///   name: string        # required string field
///   age?: number        # optional number field
///   tags: string[]      # array of strings
/// ```
pub type Picoschema = serde_json::Value;

// ---------------------------------------------------------------------------
// Evals (Tier 3)
// ---------------------------------------------------------------------------

/// A single eval test case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Eval {
    /// Human-readable description of what this eval is testing.
    pub description: Option<String>,

    /// Input variables for this eval run.
    pub input: serde_json::Value,

    /// Assertions to check against the model's output.
    pub expect: Option<EvalExpectation>,
}

/// Assertions for an eval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalExpectation {
    /// The output must contain this substring.
    pub contains: Option<String>,

    /// The output must not contain this substring.
    pub not_contains: Option<String>,

    /// The output must exactly equal this string.
    pub equals: Option<String>,

    /// A JSON schema the output must validate against (if format is JSON).
    pub json_schema: Option<serde_json::Value>,

    /// Any additional custom assertions (forward-compatibility).
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Intentry-specific metadata
// ---------------------------------------------------------------------------

/// Intentry-specific frontmatter extensions under the `intentry:` key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntrEntryMeta {
    /// Searchable tags for the commons.
    pub tags: Option<Vec<String>>,

    /// SPDX license identifier for the prompt content.
    pub license: Option<String>,

    /// Fork attribution: `"<author>/<slug>@<version>"`.
    pub parent: Option<String>,

    /// ISO 8601 timestamp when this fork was created.
    pub forked_at: Option<String>,

    /// Any unknown fields are preserved.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// A non-fatal issue encountered during parsing.
///
/// Warnings do not prevent a file from being committed, but they may reduce
/// platform functionality (e.g. a missing `description` suppresses indexing).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParseWarning {
    /// Machine-readable warning code (e.g. `"missing_description"`).
    pub code: String,
    /// Human-readable message.
    pub message: String,
}

/// A fatal parse error.
#[derive(Debug, Clone, Error, PartialEq)]
pub enum ParseError {
    /// The file is not valid UTF-8.
    #[error("file is not valid UTF-8: {0}")]
    InvalidUtf8(String),

    /// The YAML frontmatter could not be parsed.
    #[error("invalid YAML frontmatter: {0}")]
    InvalidFrontmatter(String),

    /// A Tier 2 field has an invalid value (e.g. `version` is not valid semver).
    #[error("invalid field '{field}': {reason}")]
    InvalidField { field: String, reason: String },

    /// The file exceeds the 1 MB hard size limit.
    #[error("file too large: {size} bytes (max 1 MB)")]
    FileTooLarge { size: usize },
}
