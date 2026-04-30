//! Anthropic adapter — direct HTTP to `api.anthropic.com`.
//!
//! Reference: <https://docs.anthropic.com/en/api/messages>

use std::time::Instant;

use async_trait::async_trait;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::{
    error::ProviderError,
    registry::{calc_cost_usd, Provider},
    retry::with_retry,
    types::{ApiKey, FinishReason, GenerateRequest, GenerateResponse, Role},
};

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

pub struct AnthropicProvider {
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    fn resolve_key<'a>(&self, req: &'a GenerateRequest) -> Result<String, ProviderError> {
        match &req.api_key {
            ApiKey::UserSupplied(s) => Ok(s.expose_secret().to_string()),
            ApiKey::IntentryOwned => std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
                ProviderError::MissingApiKey {
                    provider: "anthropic",
                    env_var: "ANTHROPIC_API_KEY",
                }
            }),
        }
    }
}

impl Default for AnthropicProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    messages: Vec<AnthropicMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Serialize)]
struct AnthropicMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
    model: String,
}

#[derive(Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[derive(Deserialize)]
struct AnthropicError {
    error: AnthropicErrorBody,
}

#[derive(Deserialize)]
struct AnthropicErrorBody {
    message: String,
}

// ---------------------------------------------------------------------------
// Provider impl
// ---------------------------------------------------------------------------

#[async_trait]
impl Provider for AnthropicProvider {
    fn id(&self) -> &'static str {
        "anthropic"
    }

    fn supported_models(&self) -> &[&'static str] {
        &[
            "claude-opus-4-7",
            "claude-sonnet-4-6",
            "claude-haiku-4-5",
            "claude-3-5-sonnet-20241022",
            "claude-3-5-haiku-20241022",
            "claude-3-opus-20240229",
        ]
    }

    #[instrument(skip(self, req), fields(provider = "anthropic", model = %req.model))]
    async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse, ProviderError> {
        with_retry("anthropic", &req, || self.do_generate(&req)).await
    }

    fn estimate_cost_usd(&self, model: &str, tokens_in: u32, tokens_out: u32) -> Option<f64> {
        calc_cost_usd(model, tokens_in, tokens_out)
    }
}

impl AnthropicProvider {
    async fn do_generate(&self, req: &GenerateRequest) -> Result<GenerateResponse, ProviderError> {
        let api_key = self.resolve_key(req)?;
        let started = Instant::now();

        // Split system message out.
        let system: Option<&str> = req
            .messages
            .iter()
            .find(|m| m.role == Role::System)
            .map(|m| m.content.as_str());

        let messages: Vec<AnthropicMessage<'_>> = req
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| AnthropicMessage {
                role: match m.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::System => "user", // filtered above, won't reach here
                },
                content: &m.content,
            })
            .collect();

        let body = AnthropicRequest {
            model: &req.model,
            messages,
            system,
            max_tokens: req.max_tokens.unwrap_or(4096),
            temperature: req.temperature,
        };

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .timeout(std::time::Duration::from_millis(req.timeout_ms as u64))
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Unavailable {
                provider: "anthropic",
                message: e.to_string(),
            })?;

        let status = resp.status();
        let latency_ms = started.elapsed().as_millis() as u32;

        if status == 429 {
            return Err(ProviderError::RateLimited { provider: "anthropic" });
        }
        if status.is_server_error() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Unavailable {
                provider: "anthropic",
                message: format!("HTTP {status}: {text}"),
            });
        }
        if status == 401 || status == 403 {
            let err: AnthropicError = resp.json().await.map_err(|e| ProviderError::ParseError {
                provider: "anthropic",
                message: e.to_string(),
            })?;
            return Err(ProviderError::AuthError {
                provider: "anthropic",
                message: err.error.message,
            });
        }
        if status.is_client_error() {
            let err: AnthropicError = resp.json().await.map_err(|e| ProviderError::ParseError {
                provider: "anthropic",
                message: e.to_string(),
            })?;
            return Err(ProviderError::BadRequest {
                provider: "anthropic",
                message: err.error.message,
            });
        }

        let raw_bytes = resp.bytes().await.map_err(|e| ProviderError::Unavailable {
            provider: "anthropic",
            message: e.to_string(),
        })?;

        let raw: serde_json::Value =
            serde_json::from_slice(&raw_bytes).map_err(|e| ProviderError::ParseError {
                provider: "anthropic",
                message: e.to_string(),
            })?;

        let parsed: AnthropicResponse =
            serde_json::from_value(raw.clone()).map_err(|e| ProviderError::ParseError {
                provider: "anthropic",
                message: e.to_string(),
            })?;

        let text = parsed
            .content
            .into_iter()
            .filter(|c| c.kind == "text")
            .filter_map(|c| c.text)
            .collect::<Vec<_>>()
            .join("");

        let finish_reason = match parsed.stop_reason.as_deref() {
            Some("end_turn") | Some("stop_sequence") => FinishReason::Stop,
            Some("max_tokens") => FinishReason::MaxTokens,
            Some("content_filter") => FinishReason::ContentFilter,
            Some(other) => FinishReason::Other(other.to_owned()),
            None => FinishReason::Stop,
        };

        Ok(GenerateResponse {
            text,
            finish_reason,
            tokens_in: parsed.usage.input_tokens,
            tokens_out: parsed.usage.output_tokens,
            model_used: parsed.model,
            latency_ms,
            raw_response: raw,
        })
    }
}
