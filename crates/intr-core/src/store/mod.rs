use crate::{
    error::VersionStoreError,
    events::{Event, EventCursor},
    ids::{AccountId, CommitId, PromptId, SpaceId},
    types::{Commit, Prompt, Space},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// VersionStore trait
// ---------------------------------------------------------------------------

/// The single interface every storage backend must implement.
///
/// Implementations:
/// - [`crate::local::LocalStore`] - SQLite + local filesystem (CLI, offline).
/// - `RemoteStore` (Phase 3) - PostgreSQL + Cloudflare R2 (cloud API).
///
/// All `async fn` in traits are stable since Rust 1.75. This trait is NOT
/// object-safe by design - use generics (`impl VersionStore`) or associated
/// types, not `dyn VersionStore`.
#[allow(async_fn_in_trait)]
pub trait VersionStore {
    // -- Spaces -------------------------------------------------------------

    async fn create_space(
        &self,
        input: CreateSpaceInput,
    ) -> Result<Space, VersionStoreError>;

    async fn get_space(
        &self,
        id: &SpaceId,
    ) -> Result<Space, VersionStoreError>;

    async fn get_space_by_slug(
        &self,
        owner_id: &AccountId,
        slug: &str,
    ) -> Result<Space, VersionStoreError>;

    // -- Prompts ------------------------------------------------------------

    /// Create the first commit for a new prompt in a Space.
    async fn create_prompt(
        &self,
        input: CommitInput,
    ) -> Result<Commit, VersionStoreError>;

    /// Append a new commit to an existing prompt.
    async fn commit_prompt(
        &self,
        input: CommitInput,
    ) -> Result<Commit, VersionStoreError>;

    /// Fork a prompt from another Space/prompt into this one.
    async fn fork_prompt(
        &self,
        input: ForkInput,
    ) -> Result<Commit, VersionStoreError>;

    async fn get_prompt(
        &self,
        id: &PromptId,
    ) -> Result<Prompt, VersionStoreError>;

    async fn get_prompt_by_slug(
        &self,
        space_id: &SpaceId,
        slug: &str,
    ) -> Result<Prompt, VersionStoreError>;

    async fn list_prompts(
        &self,
        space_id: &SpaceId,
        filter: PromptFilter,
        page: PageRequest,
    ) -> Result<Page<Prompt>, VersionStoreError>;

    // -- Commits ------------------------------------------------------------

    async fn get_commit(
        &self,
        id: &CommitId,
    ) -> Result<Commit, VersionStoreError>;

    async fn list_commits(
        &self,
        prompt_id: &PromptId,
        page: PageRequest,
    ) -> Result<Page<Commit>, VersionStoreError>;

    // -- Raw blob -----------------------------------------------------------

    /// Store the raw bytes of a `.prompt` file. Returns the content hash.
    async fn put_blob(
        &self,
        bytes: &[u8],
    ) -> Result<crate::ids::ContentHash, VersionStoreError>;

    /// Retrieve the raw bytes for a given content hash.
    async fn get_blob(
        &self,
        hash: &crate::ids::ContentHash,
    ) -> Result<Vec<u8>, VersionStoreError>;

    // -- Event log ----------------------------------------------------------

    async fn list_events(
        &self,
        space_id: &SpaceId,
        from: EventCursor,
        limit: u32,
    ) -> Result<Vec<Event>, VersionStoreError>;
}

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

/// Input for creating a new Space.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSpaceInput {
    pub owner_id: AccountId,
    pub slug: String,
    pub description: Option<String>,
    pub is_public: bool,
}

/// Input for creating or appending a commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitInput {
    pub space_id: SpaceId,
    pub author_id: AccountId,
    /// Target prompt: `None` when creating the first commit of a new prompt.
    pub prompt_id: Option<PromptId>,
    /// Slug used when creating a new prompt.
    pub slug: Option<String>,
    /// Raw `.prompt` file bytes.
    pub raw_bytes: Vec<u8>,
    /// Optional human-readable change message.
    pub message: Option<String>,
    /// Version bump strategy.
    pub bump: crate::version::BumpKind,
}

/// Input for forking a prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkInput {
    /// Target Space to fork into.
    pub target_space_id: SpaceId,
    pub author_id: AccountId,
    /// New slug in the target Space.
    pub new_slug: String,
    /// Source commit to fork from.
    pub source_commit_id: CommitId,
}

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

/// Cursor-based pagination request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageRequest {
    /// Opaque cursor from a previous [`Page::next_cursor`]. `None` starts from the beginning.
    pub cursor: Option<String>,
    /// Maximum items to return. Capped at 100 by each implementation.
    pub limit: u32,
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            cursor: None,
            limit: 20,
        }
    }
}

/// A page of results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// `None` when there are no more pages.
    pub next_cursor: Option<String>,
    pub total_count: Option<u64>,
}

// ---------------------------------------------------------------------------
// Filters
// ---------------------------------------------------------------------------

/// Filters for listing prompts within a Space.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptFilter {
    /// Only return prompts with all of these tags.
    pub tags: Option<Vec<String>>,
    /// Full-text search over slug and description.
    pub query: Option<String>,
    /// If `true`, include archived prompts.
    pub include_archived: bool,
    /// Only return prompts updated after this timestamp.
    pub updated_after: Option<DateTime<Utc>>,
}
