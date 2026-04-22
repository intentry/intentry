use thiserror::Error;

/// Errors returned by any [`super::VersionStore`] implementation.
#[derive(Debug, Error)]
pub enum VersionStoreError {
    /// The requested entity does not exist.
    #[error("not found: {0}")]
    NotFound(String),

    /// The operation would produce a conflict (e.g. duplicate slug, version regression).
    #[error("conflict: {0}")]
    Conflict(String),

    /// The caller lacks permission to perform this operation.
    #[error("permission denied: {0}")]
    PermissionDenied(String),

    /// A validation rule was violated (e.g. slug too long, invalid semver).
    #[error("validation error: {0}")]
    Validation(String),

    /// An unexpected error from the underlying storage backend.
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
}

/// Low-level storage backend errors.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("SQLite error: {0}")]
    Sqlite(String),

    #[error("PostgreSQL error: {0}")]
    Postgres(String),

    #[error("R2 object storage error: {0}")]
    R2(String),

    #[error("blob store error: {0}")]
    BlobStore(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("configuration error: {0}")]
    Configuration(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
