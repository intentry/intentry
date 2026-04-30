use std::process;
use thiserror::Error;

use owo_colors::OwoColorize;

use crate::ui::output::is_tty;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// CLI-level error type — maps to exit codes per V1-003 spec.
///
/// All variants ultimately render through [`CliError::exit`] which formats
/// them using the structured error template:
///
/// ```text
/// error: <message>
///   code: error.code.dotted
///   hint: one-line actionable suggestion
///   docs: https://docs.intentry.dev/errors/<anchor>
/// ```
#[derive(Debug, Error)]
pub enum CliError {
    /// General-purpose error without extra metadata.
    #[error("{0}")]
    Generic(String),

    /// Usage / argument error (exits 2).
    #[error("{0}")]
    Usage(String),

    /// Authentication required or token invalid (exits 3).
    #[error("{0}")]
    Auth(String),

    /// Network / HTTP error (exits 4).
    #[error("{0}")]
    Network(String),

    /// Validation / parse error (exits 5).
    #[error("{0}")]
    Validation(String),

    /// Fully structured error with code, hint and docs anchor.
    #[error("{message}")]
    Rich {
        message: String,
        code: &'static str,
        hint: Option<&'static str>,
        /// Anchor appended to `https://intentry.dev/docs/errors/`.
        docs: Option<&'static str>,
    },

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
    // ------------------------------------------------------------------
    // Convenience constructors
    // ------------------------------------------------------------------

    /// Build a [`CliError::Rich`] error inline.
    pub fn rich(
        message: impl Into<String>,
        code: &'static str,
        hint: Option<&'static str>,
        docs: Option<&'static str>,
    ) -> Self {
        Self::Rich { message: message.into(), code, hint, docs }
    }

    // ------------------------------------------------------------------
    // Exit codes (V1-003 spec)
    // ------------------------------------------------------------------

    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            Self::Auth(_) => 3,
            Self::Network(_) => 4,
            Self::Validation(_) => 5,
            Self::Rich { code, .. } if code.starts_with("auth.") => 3,
            Self::Rich { code, .. } if code.starts_with("network.") => 4,
            Self::Rich { code, .. } if code.starts_with("usage.") => 2,
            _ => 1,
        }
    }

    // ------------------------------------------------------------------
    // Rendering
    // ------------------------------------------------------------------

    /// Render this error to stderr in the structured format and `exit`.
    pub fn exit(self) -> ! {
        let tty = is_tty();

        match &self {
            Self::Rich { message, code, hint, docs } => {
                if tty {
                    eprintln!("{} {}", "error:".red().bold(), message);
                    eprintln!("  {}  {}", "code:".dimmed(), code.dimmed());
                    if let Some(h) = hint {
                        eprintln!("  {}  {h}", "hint:".yellow());
                    }
                    if let Some(anchor) = docs {
                        eprintln!(
                            "  {}  https://docs.intentry.dev/errors/{anchor}",
                            "docs:".dimmed()
                        );
                    }
                } else {
                    eprintln!("error: {message}");
                    eprintln!("  code: {code}");
                    if let Some(h) = hint {
                        eprintln!("  hint: {h}");
                    }
                    if let Some(anchor) = docs {
                        eprintln!("  docs: https://docs.intentry.dev/errors/{anchor}");
                    }
                }
            }
            _ => {
                if tty {
                    eprintln!("{} {}", "error:".red().bold(), self);
                } else {
                    eprintln!("error: {self}");
                }
            }
        }

        process::exit(self.exit_code())
    }
}

// ---------------------------------------------------------------------------
// Commonly-reused rich errors (called from multiple commands)
// ---------------------------------------------------------------------------

impl CliError {
    pub fn not_logged_in() -> Self {
        Self::rich(
            "not authenticated — run `intr login` first",
            "auth.not_logged_in",
            Some("run `intr login` to open the browser auth flow"),
            Some("auth-not-logged-in"),
        )
    }

    pub fn no_space() -> Self {
        Self::rich(
            "not an Intentry space (no .intr/ found)",
            "workspace.not_found",
            Some("run `intr init` to initialise a new space in this directory"),
            Some("workspace-not-found"),
        )
    }

    pub fn prompt_not_found(slug: &str) -> Self {
        Self::rich(
            format!("prompt '{slug}' not found"),
            "prompt.not_found",
            Some("run `intr list` to see available prompts"),
            Some("prompt-not-found"),
        )
    }

    pub fn version_conflict(version: &str) -> Self {
        Self::rich(
            format!("commit refused — version {version} already exists"),
            "commit.version_conflict",
            Some("use --bump minor or --version <x.y.z> to specify a different version"),
            Some("commit-version-conflict"),
        )
    }
}

pub type CliResult<T> = Result<T, CliError>;
