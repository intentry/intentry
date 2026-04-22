use thiserror::Error;

/// Errors that can occur during a diff operation.
#[derive(Debug, Error)]
pub enum DiffError {
    /// Failed to serialise/deserialise YAML or JSON values during comparison.
    #[error("serialisation error: {0}")]
    Serialisation(String),

    /// One or both inputs could not be parsed as `.prompt` files.
    /// The diff will fall back to plain-text comparison when this occurs, but
    /// callers may still want to log the warning.
    #[error("parse warning: {0}")]
    ParseWarning(String),
}
