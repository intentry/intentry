//! `intr push` - push local events to the remote.
//!
//! Reads the local event log from the `.intr/` directory and uploads any events
//! that haven't been synced yet to the cloud API via `POST /v1/events/batch`.

use crate::{
    auth,
    client::{IntrClient, PushEventItem},
    config::Config,
    error::CliResult,
    store::SpaceCtx,
    ui::output,
};
use intr_core::events::EventCursor;
use intr_core::store::VersionStore;

pub async fn run(json: bool) -> CliResult<()> {
    let config = Config::load()?;
    let token = auth::require_token()?;
    let client = IntrClient::new(&config, token);

    // Open the local space store.
    let ctx = SpaceCtx::open().await?;
    let space = &ctx.space;

    output::print_info(&format!("Pushing events for space `{}`…", space.slug));

    // Read local events from the store.
    let events = ctx
        .store
        .list_events(&space.id, EventCursor::from_start(), 1000)
        .await
        .map_err(|e| crate::error::CliError::Generic(format!("failed to read local events: {e}")))?;

    if events.is_empty() {
        if json {
            output::print_json_ok(&serde_json::json!({ "pushed": 0 }));
        } else {
            output::print_info("Nothing to push.");
        }
        return Ok(());
    }

    let count = events.len();
    let items: Vec<PushEventItem> = events
        .into_iter()
        .map(|e| {
            let payload_val = serde_json::to_value(&e.payload).ok();
            let event_type = payload_val
                .as_ref()
                .and_then(|v| v.get("type"))
                .and_then(|t| t.as_str())
                .unwrap_or("unknown")
                .to_owned();
            PushEventItem {
                event_type,
                space_id: Some(e.space_id.to_string()),
                prompt_id: None,
                payload: payload_val,
            }
        })
        .collect();

    client.push_events(items).await?;

    if json {
        output::print_json_ok(&serde_json::json!({ "pushed": count }));
    } else {
        output::print_success(&format!("Pushed {count} event{}.", if count == 1 { "" } else { "s" }));
    }
    Ok(())
}
