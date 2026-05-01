//! `intr-providers` - model-provider abstraction for Intentry.
//!
//! Every AI model call goes through the [`Provider`] trait.  To add a new
//! model you add a new adapter - nothing else changes.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use intr_providers::{ProviderRegistry, GenerateRequest, ApiKey, Message, Role};
//!
//! # #[tokio::main] async fn main() -> anyhow::Result<()> {
//! let registry = ProviderRegistry::default();
//!
//! let resp = registry
//!     .for_model("claude-sonnet-4-6")
//!     .unwrap()
//!     .generate(GenerateRequest {
//!         model: "claude-sonnet-4-6".into(),
//!         messages: vec![Message { role: Role::User, content: "Hello!".into() }],
//!         api_key: ApiKey::UserSupplied("sk-ant-...".to_string().into()),
//!         ..Default::default()
//!     })
//!     .await?;
//!
//! println!("{}", resp.text);
//! # Ok(())
//! # }
//! ```

pub mod error;
pub mod registry;
pub mod retry;
pub mod types;

pub mod providers {
    pub mod anthropic;
    pub mod google;
    pub mod mock;
    pub mod ollama;
    pub mod openai;
}

pub use error::ProviderError;
pub use registry::ProviderRegistry;
pub use types::{
    ApiKey, FinishReason, GenerateRequest, GenerateResponse, Message, Role,
};

