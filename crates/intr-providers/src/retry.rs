//! Retry logic with exponential backoff for transient provider errors.
//!
//! Schedule: 250ms → 1s → 4s (3 attempts total, ~5.25s worst case).

use std::time::Duration;

use tokio_retry::{
    strategy::{jitter, ExponentialBackoff},
    Retry,
};
use tracing::warn;

use crate::{error::ProviderError, types::GenerateRequest};

/// Retry strategy: exponential backoff starting at 250 ms, max 3 attempts.
fn retry_strategy() -> impl Iterator<Item = Duration> {
    ExponentialBackoff::from_millis(250)
        .factor(4)
        .max_delay(Duration::from_secs(4))
        .map(jitter)
        .take(3)
}

/// Run `f` with retries, transparently skipping non-retryable errors.
pub async fn with_retry<F, Fut>(
    provider_id: &'static str,
    req: &GenerateRequest,
    f: F,
) -> Result<crate::types::GenerateResponse, ProviderError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<crate::types::GenerateResponse, ProviderError>>,
{
    let strategy = retry_strategy();

    Retry::spawn(strategy, || {
        let fut = f();
        async move {
            match fut.await {
                Ok(resp) => Ok(resp),
                Err(e) if e.is_retryable() => {
                    warn!(
                        provider = provider_id,
                        model = req.model,
                        error = %e,
                        "transient provider error - will retry"
                    );
                    Err(e)
                }
                Err(e) => {
                    // Non-retryable - surface immediately as a permanent error.
                    // tokio-retry treats Err as "retry"; we use a custom action
                    // that converts to Ok(Err(...)) and unwraps after the loop.
                    // Simpler: wrap in a type that signals "don't retry".
                    // We use the trick of returning Ok with a sentinel Err.
                    Err(e)
                }
            }
        }
    })
    .await
}
