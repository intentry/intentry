//! Provider registry — resolves a model ID to the right [`Provider`] adapter.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;

use crate::{
    error::ProviderError,
    providers::{anthropic::AnthropicProvider, google::GoogleProvider, ollama::OllamaProvider, openai::OpenAIProvider},
    types::{GenerateRequest, GenerateResponse},
};

// ---------------------------------------------------------------------------
// Provider trait
// ---------------------------------------------------------------------------

/// Every model backend implements this trait.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Short stable identifier, e.g. `"anthropic"`.
    fn id(&self) -> &'static str;

    /// List of model IDs this provider handles.
    fn supported_models(&self) -> &[&'static str];

    /// Non-streaming generation.
    async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse, ProviderError>;

    /// Estimate cost in USD. Returns `None` if not applicable (e.g. Ollama).
    fn estimate_cost_usd(&self, model: &str, tokens_in: u32, tokens_out: u32) -> Option<f64>;
}

// ---------------------------------------------------------------------------
// Pricing table (USD per 1M tokens, hand-updated per release)
// ---------------------------------------------------------------------------

/// `(model_prefix_or_exact, input_per_1m_usd, output_per_1m_usd)`
pub const PRICES: &[(&str, f64, f64)] = &[
    ("claude-opus-4-7",     15.00, 75.00),
    ("claude-sonnet-4-6",    3.00, 15.00),
    ("claude-haiku-4-5",     0.80,  4.00),
    ("gpt-4o-mini",          0.15,  0.60),
    ("gpt-4o",               2.50, 10.00),
    ("gpt-4-turbo",         10.00, 30.00),
    ("gemini-2.5-pro",       1.25,  5.00),
    ("gemini-2.5-flash",     0.15,  0.60),
];

pub fn lookup_price(model: &str) -> Option<(f64, f64)> {
    PRICES
        .iter()
        .find(|(prefix, _, _)| model.starts_with(prefix) || model == *prefix)
        .map(|(_, i, o)| (*i, *o))
}

pub fn calc_cost_usd(model: &str, tokens_in: u32, tokens_out: u32) -> Option<f64> {
    let (price_in, price_out) = lookup_price(model)?;
    let cost = (tokens_in as f64 / 1_000_000.0) * price_in
        + (tokens_out as f64 / 1_000_000.0) * price_out;
    Some(cost)
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

pub struct ProviderRegistry {
    /// Map from model prefix/ID → provider.
    providers: Vec<Arc<dyn Provider>>,
    /// Direct model-id → provider for fast lookup (populated at construction).
    index: HashMap<String, Arc<dyn Provider>>,
}

impl ProviderRegistry {
    fn new() -> Self {
        Self {
            providers: Vec::new(),
            index: HashMap::new(),
        }
    }

    fn register(&mut self, p: Arc<dyn Provider>) {
        for model in p.supported_models() {
            self.index.insert(model.to_string(), Arc::clone(&p));
        }
        self.providers.push(p);
    }

    /// Build a registry with all four built-in providers.
    ///
    /// API keys are resolved from environment variables at call time, not here.
    /// The registry itself is cheap to clone (all fields are `Arc`-backed).
    pub fn default() -> Self {
        let mut r = Self::new();
        r.register(Arc::new(AnthropicProvider::new()));
        r.register(Arc::new(OpenAIProvider::new()));
        r.register(Arc::new(GoogleProvider::new()));
        r.register(Arc::new(OllamaProvider::new()));
        r
    }

    /// Resolve a model ID to its provider.
    ///
    /// Exact match first, then prefix match (e.g. "claude-" → Anthropic).
    pub fn for_model(&self, model: &str) -> Option<Arc<dyn Provider>> {
        // 1. Exact match in index.
        if let Some(p) = self.index.get(model) {
            return Some(Arc::clone(p));
        }
        // 2. Prefix match across registered providers.
        for p in &self.providers {
            for supported in p.supported_models() {
                if model.starts_with(supported) || supported.starts_with(model) {
                    return Some(Arc::clone(p));
                }
            }
        }
        // 3. Prefix-based heuristic fallback.
        if model.starts_with("claude") {
            return self.providers.iter().find(|p| p.id() == "anthropic").cloned();
        }
        if model.starts_with("gpt") || model.starts_with("o1") || model.starts_with("o3") {
            return self.providers.iter().find(|p| p.id() == "openai").cloned();
        }
        if model.starts_with("gemini") {
            return self.providers.iter().find(|p| p.id() == "google").cloned();
        }
        // Assume any other model is served by Ollama (local).
        self.providers.iter().find(|p| p.id() == "ollama").cloned()
    }

    /// Generate with ordered model fallback.
    ///
    /// Tries each model in `preferred` in order.  Falls through on retryable
    /// errors (rate limits, 5xx).  Returns the first successful response or the
    /// last error if all models fail.
    pub async fn generate_with_fallback(
        &self,
        preferred: &[String],
        req: GenerateRequest,
    ) -> Result<GenerateResponse, ProviderError> {
        let mut last_err: Option<ProviderError> = None;

        for model in preferred {
            let provider = match self.for_model(model) {
                Some(p) => p,
                None => continue,
            };

            let mut model_req = req.clone();
            model_req.model = model.clone();

            match provider.generate(model_req).await {
                Ok(resp) => return Ok(resp),
                Err(e) if e.is_retryable() => {
                    tracing::warn!(model, error = %e, "model failed, trying next in fallback list");
                    last_err = Some(e);
                }
                Err(e) => return Err(e),
            }
        }

        Err(last_err.unwrap_or_else(|| {
            ProviderError::Other(anyhow::anyhow!("no models available in fallback list"))
        }))
    }
}
