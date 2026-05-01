use intr_core::store::{PageRequest, VersionStore};

use crate::{
    error::CliResult,
    store::SpaceCtx,
    ui::output,
};

/// List all prompts in the current space.
pub async fn run(json: bool) -> CliResult<()> {
    let ctx = SpaceCtx::open().await?;

    let page = ctx
        .store
        .list_prompts(&ctx.space.id, Default::default(), PageRequest::default())
        .await
        .map_err(|e| crate::error::CliError::Generic(e.to_string()))?;

    if page.items.is_empty() {
        if json {
            output::print_json_ok(&serde_json::json!({ "prompts": [] }));
        } else {
            output::print_info("no prompts yet - run `intr new <slug>` to create one");
        }
        return Ok(());
    }

    if json {
        output::print_json_ok(&page.items);
    } else {
        // Column header
        println!("{:<32} {:>8}  {}", "SLUG", "VERSION", "UPDATED");
        println!("{}", "-".repeat(60));
        for p in &page.items {
            println!(
                "{:<32} {:>8}  {}",
                p.slug,
                p.current_version,
                p.updated_at.format("%Y-%m-%d %H:%M")
            );
        }
        if page.next_cursor.is_some() {
            output::print_info("(more results - pagination not yet supported in CLI)");
        }
    }

    Ok(())
}

