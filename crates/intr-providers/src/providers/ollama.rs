//! Ollama adapter — direct HTTP to a local (or remote) Ollama server.
//!
//! Reference: <https://github.com/ollama/ollama/blob/main/docs/api.md>
//!
//! Default base URL: `http://localhost:11434`
//! Override via the `OLLAMA_BASE_URL` environment variable.

use std::time::Instant;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::{
    error::ProviderError,
    registry::Provider,
    retry::with_retry,
    types::{FinishReason, GenerateRequest, GenerateResponse, Role},
};

const DEFAULT_BASE_URL: &str = "http://localhost:11434";

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

pub struct OllamaProvider {
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    fn base_url(&self) -> String {
        std::env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned())
    }
}

impl Default for OllamaProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct OllamaRequest<'a> {
    model: &'a str,
    messages: Vec<OllamaMessage<'a>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
}

#[derive(Serialize)]
struct OllamaMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
}

#[derive(Deserialize)]
struct OllamaResponse {
    message: OllamaMessage2,
    done_reason: Option<String>,
    #[serde(rename = "prompt_eval_count")]
    prompt_eval_count: Option<u32>,
    #[serde(rename = "eval_count")]
    eval_count: Option<u32>,
    model: String,
}

#[derive(Deserialize)]
struct OllamaMessage2 {
    content: String,
}

// ---------------------------------------------------------------------------
// Provider impl
// ---------------------------------------------------------------------------

#[async_trait]
impl Provider for OllamaProvider {
    fn id(&self) -> &'static str {
        "ollama"
    }

    /// Ollama supports any model pulled locally — we use a sensible default
    /// list for the index but fall through by prefix for anything else.
    fn supported_models(&self) -> &[&'static str] {
        &[
            "llama3",
            "llama3.1",
            "llama3.2",
            "llama3.3",
            "mistral",
            "mistral-nemo",
            "phi4",
            "phi4-mini",
            "gemma3",
            "qwen3",
            "deepseek-r1",
        ]
    }

    #[instrument(skip(self, req), fields(provider = "ollama", model = %req.model))]
    async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse, ProviderError> {
        with_retry("ollama", &req, || self.do_generate(&req)).await
    }

    /// Ollama is free (local inference) — cost is always `None`.
    fn estimate_cost_usd(&self, _model: &str, _tokens_in: u32, _tokens_out: u32) -> Option<f64> {
        None
    }
}

impl OllamaProvider {
    async fn do_generate(&self, req: &GenerateRequest) -> Result<GenerateResponse, ProviderError> {
        let started = Instant::now();

        let messages: Vec<OllamaMessage<'_>> = req
            .messages
            .iter()
            .map(|m| OllamaMessage {
                role: match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                },
                content: &m.content,
            })
            .collect();

        let body = OllamaRequest {
            model: &req.model,
            messages,
            stream: false,
            options: if req.temperature.is_some() || req.max_tokens.is_some() {
                Some(OllamaOptions {
                    temperature: req.temperature,
                    num_predict: req.max_tokens,
                })
            } else {
                None
            },
        };

        let url = format!("{}/api/chat", self.base_url());

        let resp = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .timeout(std::time::Duration::from_millis(req.timeout_ms as u64))
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Unavailable {
                provider: "ollama",
                message: e.to_string(),
            })?;

        let status = resp.status();
        let latency_ms = started.elapsed().as_millis() as u32;

        if status == 429 {
            return Err(ProviderError::RateLimited { provider: "ollama" });
        }
        if status.is_server_error() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Unavailable {
                provider: "ollama",
                message: format!("HTTP {status}: {text}"),
            });
        }
        if status.is_client_error() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::BadRequest {
                provider: "ollama",
                message: format!("HTTP {status}: {text}"),
            });
        }

        let raw_bytes = resp.bytes().await.map_err(|e| ProviderError::Unavailable {
            provider: "ollama",
            message: e.to_string(),
        })?;

        let raw: serde_json::Value =
            serde_json::from_slice(&raw_bytes).map_err(|e| ProviderError::ParseError {
                provider: "ollama",
                message: e.to_string(),
            })?;

        let parsed: OllamaResponse =
            serde_json::from_value(raw.clone()).map_err(|e| ProviderError::ParseError {
                provider: "ollama",
                message: e.to_string(),
            })?;

        let finish_reason = match parsed.done_reason.as_deref() {
            Some("stop") | Some("eos") => FinishReason::Stop,
            Some("length") => FinishReason::MaxTokens,
            Some(other) => FinishReason::Other(other.to_owned()),
            None => FinishReason::Stop,
        };

        Ok(GenerateResponse {
            text: parsed.message.content,
            finish_reason,
            tokens_in: parsed.prompt_eval_count.unwrap_or(0),
            tokens_out: parsed.eval_count.unwrap_or(0),
            model_used: parsed.model,
            latency_ms,
            raw_response: raw,
        })
    }
}
