use crate::ids::{AccountId, CommitId, ContentHash, PromptId, SpaceId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Event envelope
// ---------------------------------------------------------------------------

/// The full event log entry as stored in the append-only events table.
///
/// `payload` is the discriminated union of all domain events. Every write to
/// the system produces exactly one event; state is always derived by replaying
/// the event log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Monotonically increasing sequence number within a Space.
    pub seq: u64,
    pub occurred_at: DateTime<Utc>,
    pub actor_id: AccountId,
    pub space_id: SpaceId,
    pub payload: EventPayload,
}

/// Cursor for paginating through the event log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventCursor {
    pub seq: u64,
}

impl EventCursor {
    pub fn from_start() -> Self {
        Self { seq: 0 }
    }
}

// ---------------------------------------------------------------------------
// Event payloads
// ---------------------------------------------------------------------------

/// All domain event variants.
///
/// Variants are named in past-tense imperative (`VerbNounDone`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventPayload {
    // -- Space events -------------------------------------------------------
    SpaceCreated {
        space_id: SpaceId,
        slug: String,
        is_public: bool,
    },
    SpaceDescriptionUpdated {
        space_id: SpaceId,
        description: String,
    },
    SpaceVisibilityChanged {
        space_id: SpaceId,
        is_public: bool,
    },

    // -- Prompt events ------------------------------------------------------
    /// First commit in a prompt's history.
    PromptCreated {
        prompt_id: PromptId,
        space_id: SpaceId,
        slug: String,
        commit_id: CommitId,
        content_hash: ContentHash,
        version: semver::Version,
        message: Option<String>,
    },
    /// Subsequent commit to an existing prompt.
    PromptCommitted {
        prompt_id: PromptId,
        space_id: SpaceId,
        commit_id: CommitId,
        parent_commit_id: CommitId,
        content_hash: ContentHash,
        version: semver::Version,
        message: Option<String>,
    },
    /// Prompt forked from another Space/prompt.
    PromptForked {
        prompt_id: PromptId,
        space_id: SpaceId,
        slug: String,
        commit_id: CommitId,
        content_hash: ContentHash,
        version: semver::Version,
        parent_prompt_id: PromptId,
        parent_space_id: SpaceId,
        parent_version: semver::Version,
    },
    /// Prompt permanently deleted (soft-delete; events are retained).
    PromptArchived {
        prompt_id: PromptId,
        space_id: SpaceId,
    },
}
