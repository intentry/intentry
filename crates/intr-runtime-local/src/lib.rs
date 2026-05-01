//! `intr-runtime-local` - Execute `.prompt` files against model providers locally.
//!
//! API keys are resolved from environment variables at call time:
//! - `ANTHROPIC_API_KEY` - Claude models
//! - `OPENAI_API_KEY`    - GPT / o* models
//! - `GOOGLE_API_KEY`    - Gemini models
//! - *(no key needed)*   - Ollama (local inference)

pub mod error;

pub use error::RuntimeError;

use handlebars::Handlebars;
use intr_parser::parse;
use intr_providers::{ApiKey, GenerateRequest, Message, ProviderRegistry, Role};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Input to a single prompt execution.
#[derive(Debug)]
pub struct RunInput {
    /// Full source content of the `.prompt` file.
    pub prompt_content: String,
    /// Template variables as a JSON object.  Use `serde_json::json!({})` when
    /// the prompt has no variables.
    pub variables: Value,
    /// Override the model chosen by frontmatter.
    pub model_override: Option<String>,
}

/// Successful output from a prompt execution.
#[derive(Debug, Clone)]
pub struct RunOutput {
    /// The generated text.
    pub text: String,
    /// Actual model ID that produced the response.
    pub model_used: String,
    /// Input tokens consumed.
    pub tokens_in: u32,
    /// Output tokens generated.
    pub tokens_out: u32,
    /// Wall-clock latency in milliseconds.
    pub latency_ms: u32,
    /// Estimated cost in USD, if pricing data is available.
    pub cost_usd: Option<f64>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Execute a `.prompt` file against the appropriate model provider.
///
/// Model resolution order:
/// 1. `input.model_override`
/// 2. `model.preferred[0]` from frontmatter
/// 3. `INTR_DEFAULT_MODEL` environment variable
/// 4. `"claude-sonnet-4-6"` (final fallback)
pub async fn run(input: RunInput) -> Result<RunOutput, RuntimeError> {
    // 1. Parse.
    let parsed = parse(input.prompt_content.as_bytes())
        .map_err(|e| RuntimeError::Parse(e.to_string()))?;

    // 2. Render Handlebars template.
    let rendered = render_template(&parsed.body, &input.variables)?;

    // 3. Resolve preferred model list.
    let preferred: Vec<String> = match (input.model_override.as_deref(), &parsed.frontmatter) {
        (Some(m), _) => vec![m.to_owned()],
        (None, Some(fm)) => fm
            .model
            .as_ref()
            .and_then(|h| h.preferred.clone())
            .unwrap_or_default(),
        (None, None) => vec![],
    };
    let preferred = if preferred.is_empty() {
        vec![std::env::var("INTR_DEFAULT_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-6".to_owned())]
    } else {
        preferred
    };

    // 4. Build messages.  Use frontmatter `description` as system prompt.
    let mut messages: Vec<Message> = Vec::new();
    if let Some(fm) = &parsed.frontmatter {
        if let Some(desc) = &fm.description {
            messages.push(Message {
                role: Role::System,
                content: desc.clone(),
            });
        }
    }
    messages.push(Message {
        role: Role::User,
        content: rendered,
    });

    // 5. Build the generation request, applying frontmatter sampling hints.
    let mut req = GenerateRequest {
        model: preferred[0].clone(),
        messages,
        api_key: ApiKey::IntentryOwned,
        ..Default::default()
    };
    if let Some(fm) = &parsed.frontmatter {
        if let Some(hints) = &fm.model {
            req.temperature = hints.temperature.map(|t| t as f32);
            req.max_tokens = hints.max_tokens;
        }
    }

    // 6. Execute, with multi-model fallback when the frontmatter lists several.
    let registry = ProviderRegistry::default();
    let resp = if preferred.len() > 1 {
        registry
            .generate_with_fallback(&preferred, req)
            .await
            .map_err(|e| RuntimeError::Provider(e.to_string()))?
    } else {
        registry
            .for_model(&preferred[0])
            .ok_or_else(|| RuntimeError::UnknownModel(preferred[0].clone()))?
            .generate(req)
            .await
            .map_err(|e| RuntimeError::Provider(e.to_string()))?
    };

    let cost_usd =
        intr_providers::registry::calc_cost_usd(&resp.model_used, resp.tokens_in, resp.tokens_out);

    Ok(RunOutput {
        text: resp.text,
        model_used: resp.model_used,
        tokens_in: resp.tokens_in,
        tokens_out: resp.tokens_out,
        latency_ms: resp.latency_ms,
        cost_usd,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn render_template(body: &str, variables: &Value) -> Result<String, RuntimeError> {
    let mut hbs = Handlebars::new();
    // Lenient mode: missing variables render as empty string (Dotprompt spec).
    hbs.set_strict_mode(false);
    hbs.render_template(body, variables)
        .map_err(|e| RuntimeError::Template(e.to_string()))
}

