use std::process;
use thiserror::Error;

/// CLI-level error type — maps to exit codes per V1-003 spec.
#[derive(Debug, Error)]
pub enum CliError {
    #[error("{0}")]
    Generic(String),

    #[error("usage: {0}")]
    Usage(String),

    #[error("authentication required: {0}")]
    Auth(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("validation error: {0}")]
    Validation(String),

    #[error(transparent)]
    Core(#[from] intr_core::VersionStoreError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl CliError {
    /// Exit code per V1-003 spec.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Generic(_) | Self::Core(_) | Self::Io(_) | Self::Json(_) | Self::Other(_) => 1,
            Self::Usage(_) => 2,
            Self::Auth(_) => 3,
            Self::Network(_) => 4,
            Self::Validation(_) => 5,
        }
    }

    /// Print this error to stderr and exit with the appropriate code.
    pub fn exit(self) -> ! {
        eprintln!("error: {self}");
        process::exit(self.exit_code())
    }
}

pub type CliResult<T> = Result<T, CliError>;
