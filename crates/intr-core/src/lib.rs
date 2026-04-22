//! `intr-core` — Domain types, events, and the [`VersionStore`] trait.
//!
//! This crate is the heart of Intentry. It defines:
//!
//! - **ID newtypes** — type-safe, prefixed UUIDv7 identifiers.
//! - **Domain types** — [`Prompt`], [`Commit`], [`Space`], [`Account`], etc.
//! - **[`ContentHash`]** — SHA-256 content-addressed blob identifier.
//! - **[`VersionStore`] trait** — the single interface every store backend must implement.
//! - **Event types** — append-only event log entries.
//! - **Error types** — [`VersionStoreError`].
//!
//! Feature flags:
//! - `local` (default) — enables the [`LocalStore`] skeleton (SQLite, offline CLI).
//! - `postgres` — enables the [`RemoteStore`] skeleton (PostgreSQL, cloud API).

pub mod error;
pub mod events;
pub mod ids;
pub mod store;
pub mod types;
pub mod version;

#[cfg(feature = "local")]
pub mod local;

pub use error::VersionStoreError;
pub use ids::{AccountId, CommitId, PromptId, RunId, SpaceId};
pub use store::VersionStore;
pub use types::{Account, Commit, Prompt, Space};
