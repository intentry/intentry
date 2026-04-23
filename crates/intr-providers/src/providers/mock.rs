//! Mock provider for tests — never makes real HTTP calls.

use async_trait::async_trait;

use crate::{
    error::ProviderError,
    registry::Provider,
    types::{FinishReason, GenerateRequest, GenerateResponse},
};

pub struct MockProvider {
    pub response_text: String,
}

impl MockProvider {
    pub fn new(response_text: impl Into<String>) -> Self {
        Self {
            response_text: response_text.into(),
        }
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn id(&self) -> &'static str {
        "mock"
    }

    fn supported_models(&self) -> &[&'static str] {
        &["mock-model"]
    }

    async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse, ProviderError> {
        Ok(GenerateResponse {
            text: self.response_text.clone(),
            finish_reason: FinishReason::Stop,
            tokens_in: 10,
            tokens_out: 20,
            model_used: req.model,
            latency_ms: 1,
            raw_response: serde_json::json!({ "mock": true }),
        })
    }

    fn estimate_cost_usd(&self, _model: &str, _tokens_in: u32, _tokens_out: u32) -> Option<f64> {
        None
    }
}
