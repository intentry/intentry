use crate::ids::{AccountId, CommitId, ContentHash, PromptId, SpaceId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Account
// ---------------------------------------------------------------------------

/// A registered user or organisation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub id: AccountId,
    pub handle: String,
    pub display_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Space
// ---------------------------------------------------------------------------

/// A Space is a named collection of prompts owned by one Account.
///
/// Equivalent to a GitHub repo in the analogy: `owner/space-name`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Space {
    pub id: SpaceId,
    pub owner_id: AccountId,
    /// URL-safe slug: alphanumeric + hyphens, max 64 chars.
    pub slug: String,
    pub description: Option<String>,
    pub is_public: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Prompt
// ---------------------------------------------------------------------------

/// A Prompt is a named, versioned `.prompt` file within a Space.
///
/// The latest commit determines the current content. Historical commits are
/// accessible via the commit log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prompt {
    pub id: PromptId,
    pub space_id: SpaceId,
    /// Stable identifier from the frontmatter `id:` field.
    pub slug: String,
    /// ID of the most recent commit.
    pub head_commit_id: CommitId,
    /// Denormalized current semver for fast lookup.
    pub current_version: semver::Version,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Commit
// ---------------------------------------------------------------------------

/// An immutable commit in a Prompt's version history.
///
/// Every commit is content-addressed; two commits with the same `content_hash`
/// have identical raw file bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Commit {
    pub id: CommitId,
    pub prompt_id: PromptId,
    pub space_id: SpaceId,
    pub author_id: AccountId,
    /// SHA-256 hash of the raw `.prompt` file bytes.
    pub content_hash: ContentHash,
    /// Parsed semver version from the frontmatter.
    pub version: semver::Version,
    /// Optional human-readable message describing the change.
    pub message: Option<String>,
    /// ID of the parent commit (None for the first commit of a prompt).
    pub parent_id: Option<CommitId>,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// ParsedMeta
// ---------------------------------------------------------------------------

/// Cached parsed frontmatter stored alongside a commit.
///
/// Populated by the store after a successful parse. Used by the API for
/// search indexing, eval scheduling, and model routing - without re-parsing
/// raw bytes on every read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedMeta {
    pub commit_id: CommitId,
    /// Detected tier (1, 2, or 3).
    pub tier: u8,
    pub description: Option<String>,
    pub preferred_models: Vec<String>,
    pub tags: Vec<String>,
    pub license: Option<String>,
    pub has_evals: bool,
    pub variables: Vec<String>,
}
