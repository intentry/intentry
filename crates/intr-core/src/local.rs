/// Stub for the local SQLite-backed store (Phase 2 implementation).
///
/// This module exists in Phase 1 as a placeholder so the public API surface
/// is established and the CLI crate can reference the type.
///
/// The `LocalStore` will be fully implemented in Phase 2 (V1-001 Phase 2 spec).
/// It uses `sqlx` with the `sqlite` feature — the same library as `RemoteStore`
/// (postgres), keeping the dependency tree minimal.

/// A local, offline-capable store backed by SQLite + the local filesystem.
///
/// All data is stored in a single SQLite database at `~/.intentry/store.db`
/// (configurable via `INTENTRY_DATA_DIR`).
///
/// This is the store implementation used by the `intr` CLI. It does NOT
/// require any network access or cloud credentials.
pub struct LocalStore {
    // Phase 2: will hold a `sqlx::SqlitePool`.
    _private: (),
}

impl LocalStore {
    /// Create or open the local store at the default path.
    ///
    /// In Phase 1 this always returns an error; implementation comes in Phase 2.
    pub fn open() -> Result<Self, crate::error::StorageError> {
        Err(crate::error::StorageError::Sqlite(
            "LocalStore is not yet implemented (Phase 2)".to_string(),
        ))
    }
}
