//! Google Gemini adapter — direct HTTP to the Google Generative Language API.
//!
//! Reference: <https://ai.google.dev/api/generate-content>

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

const BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

pub struct GoogleProvider {
    client: reqwest::Client,
}

impl GoogleProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    fn resolve_key(&self, req: &GenerateRequest) -> Result<String, ProviderError> {
        match &req.api_key {
            ApiKey::UserSupplied(s) => Ok(s.expose_secret().to_string()),
            ApiKey::IntentryOwned => std::env::var("GOOGLE_API_KEY").map_err(|_| {
                ProviderError::MissingApiKey {
                    provider: "google",
                    env_var: "GOOGLE_API_KEY",
                }
            }),
        }
    }
}

impl Default for GoogleProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiSystemInstruction>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Serialize)]
struct GeminiSystemInstruction {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize, Deserialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(rename = "maxOutputTokens", skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(rename = "responseMimeType", skip_serializing_if = "Option::is_none")]
    response_mime_type: Option<&'static str>,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsageMetadata>,
    #[serde(rename = "modelVersion")]
    model_version: Option<String>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent2>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct GeminiContent2 {
    parts: Vec<GeminiPart>,
}

#[derive(Deserialize)]
struct GeminiUsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<u32>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<u32>,
}

// ---------------------------------------------------------------------------
// Provider impl
// ---------------------------------------------------------------------------

#[async_trait]
impl Provider for GoogleProvider {
    fn id(&self) -> &'static str {
        "google"
    }

    fn supported_models(&self) -> &[&'static str] {
        &[
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.0-flash",
            "gemini-1.5-pro",
            "gemini-1.5-flash",
        ]
    }

    #[instrument(skip(self, req), fields(provider = "google", model = %req.model))]
    async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse, ProviderError> {
        with_retry("google", &req, || self.do_generate(&req)).await
    }

    fn estimate_cost_usd(&self, model: &str, tokens_in: u32, tokens_out: u32) -> Option<f64> {
        calc_cost_usd(model, tokens_in, tokens_out)
    }
}

impl GoogleProvider {
    async fn do_generate(&self, req: &GenerateRequest) -> Result<GenerateResponse, ProviderError> {
        let api_key = self.resolve_key(req)?;
        let started = Instant::now();

        let system_text: Option<&str> = req
            .messages
            .iter()
            .find(|m| m.role == Role::System)
            .map(|m| m.content.as_str());

        let contents: Vec<GeminiContent> = req
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| GeminiContent {
                role: match m.role {
                    Role::User => "user".to_owned(),
                    Role::Assistant => "model".to_owned(),
                    Role::System => "user".to_owned(),
                },
                parts: vec![GeminiPart { text: m.content.clone() }],
            })
            .collect();

        let body = GeminiRequest {
            contents,
            system_instruction: system_text.map(|t| GeminiSystemInstruction {
                parts: vec![GeminiPart { text: t.to_owned() }],
            }),
            generation_config: Some(GeminiGenerationConfig {
                temperature: req.temperature,
                max_output_tokens: req.max_tokens,
                response_mime_type: if req.json_mode {
                    Some("application/json")
                } else {
                    None
                },
            }),
        };

        let url = format!("{BASE_URL}/{}:generateContent?key={}", req.model, api_key);

        let resp = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .timeout(std::time::Duration::from_millis(req.timeout_ms as u64))
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Unavailable {
                provider: "google",
                message: e.to_string(),
            })?;

        let status = resp.status();
        let latency_ms = started.elapsed().as_millis() as u32;

        if status == 429 {
            return Err(ProviderError::RateLimited { provider: "google" });
        }
        if status.is_server_error() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Unavailable {
                provider: "google",
                message: format!("HTTP {status}: {text}"),
            });
        }
        if status == 401 || status == 403 {
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::AuthError {
                provider: "google",
                message: text,
            });
        }
        if status.is_client_error() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::BadRequest {
                provider: "google",
                message: format!("HTTP {status}: {text}"),
            });
        }

        let raw_bytes = resp.bytes().await.map_err(|e| ProviderError::Unavailable {
            provider: "google",
            message: e.to_string(),
        })?;

        let raw: serde_json::Value =
            serde_json::from_slice(&raw_bytes).map_err(|e| ProviderError::ParseError {
                provider: "google",
                message: e.to_string(),
            })?;

        let parsed: GeminiResponse =
            serde_json::from_value(raw.clone()).map_err(|e| ProviderError::ParseError {
                provider: "google",
                message: e.to_string(),
            })?;

        let candidate = parsed.candidates.into_iter().next().ok_or_else(|| {
            ProviderError::ParseError {
                provider: "google",
                message: "response had no candidates".into(),
            }
        })?;

        let text = candidate
            .content
            .map(|c| {
                c.parts
                    .into_iter()
                    .map(|p| p.text)
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();

        let finish_reason = match candidate.finish_reason.as_deref() {
            Some("STOP") => FinishReason::Stop,
            Some("MAX_TOKENS") => FinishReason::MaxTokens,
            Some("SAFETY") => FinishReason::ContentFilter,
            Some(other) => FinishReason::Other(other.to_owned()),
            None => FinishReason::Stop,
        };

        let (tokens_in, tokens_out) = parsed
            .usage_metadata
            .map(|u| {
                (
                    u.prompt_token_count.unwrap_or(0),
                    u.candidates_token_count.unwrap_or(0),
                )
            })
            .unwrap_or((0, 0));

        Ok(GenerateResponse {
            text,
            finish_reason,
            tokens_in,
            tokens_out,
            model_used: parsed.model_version.unwrap_or_else(|| req.model.clone()),
            latency_ms,
            raw_response: raw,
        })
    }
}
