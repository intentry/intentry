use intr_core::store::{PageRequest, VersionStore};

use crate::{
    error::{CliError, CliResult},
    store::SpaceCtx,
    ui::output,
};

/// Show the commit history for a prompt.
pub async fn run(prompt_slug: &str, json: bool) -> CliResult<()> {
    let ctx = SpaceCtx::open().await?;

    let prompt = ctx
        .store
        .get_prompt_by_slug(&ctx.space.id, prompt_slug)
        .await
        .map_err(|e| CliError::Generic(format!("prompt '{prompt_slug}' not found: {e}")))?;

    let page = ctx
        .store
        .list_commits(&prompt.id, PageRequest::default())
        .await
        .map_err(|e| CliError::Generic(e.to_string()))?;

    if json {
        let items: Vec<_> = page
            .items
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id.to_string(),
                    "version": c.version.to_string(),
                    "message": c.message,
                    "content_hash": c.content_hash.to_string(),
                    "created_at": c.created_at,
                })
            })
            .collect();
        output::print_json_ok(&items);
    } else {
        for commit in &page.items {
            let msg = commit.message.as_deref().unwrap_or("(no message)");
            println!(
                "  {} v{}  {}  {}",
                &commit.id.to_string()[..8],
                commit.version,
                commit.created_at.format("%Y-%m-%d %H:%M"),
                msg
            );
        }
        if page.items.is_empty() {
            output::print_info("no commits yet");
        }
    }

    Ok(())
}

