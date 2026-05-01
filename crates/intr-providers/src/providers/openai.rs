//! OpenAI adapter - direct HTTP to `api.openai.com`.
//!
//! Reference: <https://platform.openai.com/docs/api-reference/chat>

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

pub struct OpenAIProvider {
    client: reqwest::Client,
}

impl OpenAIProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    fn resolve_key(&self, req: &GenerateRequest) -> Result<String, ProviderError> {
        match &req.api_key {
            ApiKey::UserSupplied(s) => Ok(s.expose_secret().to_string()),
            ApiKey::IntentryOwned => std::env::var("OPENAI_API_KEY").map_err(|_| {
                ProviderError::MissingApiKey {
                    provider: "openai",
                    env_var: "OPENAI_API_KEY",
                }
            }),
        }
    }
}

impl Default for OpenAIProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct OpenAIRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAIMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<OpenAIResponseFormat>,
}

#[derive(Serialize)]
struct OpenAIMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct OpenAIResponseFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
    usage: Option<OpenAIUsage>,
    model: String,
}

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIChoiceMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAIChoiceMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct OpenAIUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[derive(Deserialize)]
struct OpenAIError {
    error: OpenAIErrorBody,
}

#[derive(Deserialize)]
struct OpenAIErrorBody {
    message: String,
}

// ---------------------------------------------------------------------------
// Provider impl
// ---------------------------------------------------------------------------

#[async_trait]
impl Provider for OpenAIProvider {
    fn id(&self) -> &'static str {
        "openai"
    }

    fn supported_models(&self) -> &[&'static str] {
        &[
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4-turbo",
            "gpt-3.5-turbo",
            "o1",
            "o1-mini",
            "o3",
            "o3-mini",
            "o4-mini",
        ]
    }

    #[instrument(skip(self, req), fields(provider = "openai", model = %req.model))]
    async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse, ProviderError> {
        with_retry("openai", &req, || self.do_generate(&req)).await
    }

    fn estimate_cost_usd(&self, model: &str, tokens_in: u32, tokens_out: u32) -> Option<f64> {
        calc_cost_usd(model, tokens_in, tokens_out)
    }
}

impl OpenAIProvider {
    async fn do_generate(&self, req: &GenerateRequest) -> Result<GenerateResponse, ProviderError> {
        let api_key = self.resolve_key(req)?;
        let started = Instant::now();

        let messages: Vec<OpenAIMessage<'_>> = req
            .messages
            .iter()
            .map(|m| OpenAIMessage {
                role: match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                },
                content: &m.content,
            })
            .collect();

        let body = OpenAIRequest {
            model: &req.model,
            messages,
            temperature: req.temperature,
            max_completion_tokens: req.max_tokens,
            response_format: if req.json_mode {
                Some(OpenAIResponseFormat { kind: "json_object" })
            } else {
                None
            },
        };

        let resp = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(&api_key)
            .timeout(std::time::Duration::from_millis(req.timeout_ms as u64))
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Unavailable {
                provider: "openai",
                message: e.to_string(),
            })?;

        let status = resp.status();
        let latency_ms = started.elapsed().as_millis() as u32;

        if status == 429 {
            return Err(ProviderError::RateLimited { provider: "openai" });
        }
        if status.is_server_error() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Unavailable {
                provider: "openai",
                message: format!("HTTP {status}: {text}"),
            });
        }
        if status == 401 || status == 403 {
            let err: OpenAIError = resp.json().await.map_err(|e| ProviderError::ParseError {
                provider: "openai",
                message: e.to_string(),
            })?;
            return Err(ProviderError::AuthError {
                provider: "openai",
                message: err.error.message,
            });
        }
        if status.is_client_error() {
            let err: OpenAIError = resp.json().await.map_err(|e| ProviderError::ParseError {
                provider: "openai",
                message: e.to_string(),
            })?;
            return Err(ProviderError::BadRequest {
                provider: "openai",
                message: err.error.message,
            });
        }

        let raw_bytes = resp.bytes().await.map_err(|e| ProviderError::Unavailable {
            provider: "openai",
            message: e.to_string(),
        })?;

        let raw: serde_json::Value =
            serde_json::from_slice(&raw_bytes).map_err(|e| ProviderError::ParseError {
                provider: "openai",
                message: e.to_string(),
            })?;

        let parsed: OpenAIResponse =
            serde_json::from_value(raw.clone()).map_err(|e| ProviderError::ParseError {
                provider: "openai",
                message: e.to_string(),
            })?;

        let choice = parsed.choices.into_iter().next().ok_or_else(|| {
            ProviderError::ParseError {
                provider: "openai",
                message: "response had no choices".into(),
            }
        })?;

        let text = choice.message.content.unwrap_or_default();

        let finish_reason = match choice.finish_reason.as_deref() {
            Some("stop") => FinishReason::Stop,
            Some("length") => FinishReason::MaxTokens,
            Some("content_filter") => FinishReason::ContentFilter,
            Some(other) => FinishReason::Other(other.to_owned()),
            None => FinishReason::Stop,
        };

        let (tokens_in, tokens_out) = parsed
            .usage
            .map(|u| (u.prompt_tokens, u.completion_tokens))
            .unwrap_or((0, 0));

        Ok(GenerateResponse {
            text,
            finish_reason,
            tokens_in,
            tokens_out,
            model_used: parsed.model,
            latency_ms,
            raw_response: raw,
        })
    }
}
