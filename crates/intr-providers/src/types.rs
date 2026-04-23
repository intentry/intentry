//! Shared request/response types for all providers.

use std::collections::HashMap;

use secrecy::SecretString;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

// ---------------------------------------------------------------------------
// API key
// ---------------------------------------------------------------------------

/// Represents the API key used for a model call.
///
/// Uses [`SecretString`] from the `secrecy` crate which:
/// - Never exposes the secret in `Debug` output
/// - Zeroes the memory on drop
#[derive(Clone)]
pub enum ApiKey {
    /// Caller-supplied key — passed per request, never persisted by Intentry.
    UserSupplied(SecretString),
    /// Intentry-owned key — resolved from environment at startup.
    /// (Used when Intentry runs model calls on behalf of the user.)
    IntentryOwned,
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiKey::UserSupplied(_) => write!(f, "ApiKey::UserSupplied([REDACTED])"),
            ApiKey::IntentryOwned => write!(f, "ApiKey::IntentryOwned"),
        }
    }
}

// ---------------------------------------------------------------------------
// Request
// ---------------------------------------------------------------------------

/// A model generation request, normalised across providers.
#[derive(Debug, Clone)]
pub struct GenerateRequest {
    /// Model identifier, e.g. `"claude-sonnet-4-6"` or `"gpt-4o"`.
    pub model: String,
    /// Conversation turn history.
    pub messages: Vec<Message>,
    /// Sampling temperature (0.0–1.0). `None` → provider default.
    pub temperature: Option<f32>,
    /// Maximum output tokens. `None` → provider default.
    pub max_tokens: Option<u32>,
    /// Request JSON-mode output from the provider when supported.
    pub json_mode: bool,
    /// Provider-specific extra parameters (passed through verbatim).
    pub extra: HashMap<String, serde_json::Value>,
    /// API key to use for this call.
    pub api_key: ApiKey,
    /// Per-request timeout in milliseconds. Default: 30 000 ms.
    pub timeout_ms: u32,
}

impl Default for GenerateRequest {
    fn default() -> Self {
        Self {
            model: String::new(),
            messages: Vec::new(),
            temperature: None,
            max_tokens: None,
            json_mode: false,
            extra: HashMap::new(),
            api_key: ApiKey::IntentryOwned,
            timeout_ms: 30_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Response
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishReason {
    Stop,
    MaxTokens,
    ContentFilter,
    Other(String),
}

/// A normalised generation response.
#[derive(Debug, Clone)]
pub struct GenerateResponse {
    /// Model output text (the assistant turn).
    pub text: String,
    /// Why the generation stopped.
    pub finish_reason: FinishReason,
    /// Input tokens consumed.
    pub tokens_in: u32,
    /// Output tokens generated.
    pub tokens_out: u32,
    /// Exact model ID that handled the request (provider may route).
    pub model_used: String,
    /// End-to-end wall-clock latency in milliseconds.
    pub latency_ms: u32,
    /// Raw provider JSON response (opaque; useful for debugging).
    pub raw_response: serde_json::Value,
}
