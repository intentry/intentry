//! Provider error types.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    /// HTTP 401/403 - bad API key or access denied. Do not retry.
    #[error("authentication failed for provider '{provider}': {message}")]
    AuthError { provider: &'static str, message: String },

    /// HTTP 400/422 - we sent bad input. Do not retry.
    #[error("bad request to provider '{provider}': {message}")]
    BadRequest { provider: &'static str, message: String },

    /// HTTP 429 - rate limited. Retryable.
    #[error("rate limited by provider '{provider}'")]
    RateLimited { provider: &'static str },

    /// HTTP 5xx or network/timeout error. Retryable.
    #[error("provider '{provider}' unavailable: {message}")]
    Unavailable { provider: &'static str, message: String },

    /// The provider returned a response we couldn't parse.
    #[error("provider '{provider}' returned unparseable response: {message}")]
    ParseError { provider: &'static str, message: String },

    /// No key was configured for IntentryOwned variant.
    #[error("no API key configured for provider '{provider}'; set {env_var}")]
    MissingApiKey { provider: &'static str, env_var: &'static str },

    /// Unknown / catch-all.
    #[error("provider error: {0}")]
    Other(#[from] anyhow::Error),
}

impl ProviderError {
    /// Returns `true` if the error is transient and the caller should retry.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::RateLimited { .. } | Self::Unavailable { .. })
    }
}
