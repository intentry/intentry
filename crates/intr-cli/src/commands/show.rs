use intr_core::store::VersionStore;

use crate::{
    error::{CliError, CliResult},
    store::SpaceCtx,
    ui::output,
};

/// Show details for a specific prompt.
pub async fn run(prompt_slug: &str, content: bool, json: bool) -> CliResult<()> {
    let ctx = SpaceCtx::open().await?;

    let prompt = ctx
        .store
        .get_prompt_by_slug(&ctx.space.id, prompt_slug)
        .await
        .map_err(|e| CliError::Generic(format!("prompt '{prompt_slug}' not found: {e}")))?;

    let head_commit = ctx
        .store
        .get_commit(&prompt.head_commit_id)
        .await
        .map_err(|e| CliError::Generic(e.to_string()))?;

    let raw_content = if content {
        let bytes = ctx
            .store
            .get_blob(&head_commit.content_hash)
            .await
            .map_err(|e| CliError::Generic(e.to_string()))?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    } else {
        None
    };

    if json {
        let mut obj = serde_json::json!({
            "prompt": {
                "id": prompt.id.to_string(),
                "slug": prompt.slug,
                "version": prompt.current_version.to_string(),
                "space_id": prompt.space_id.to_string(),
                "created_at": prompt.created_at,
                "updated_at": prompt.updated_at,
            },
            "head_commit": {
                "id": head_commit.id.to_string(),
                "version": head_commit.version.to_string(),
                "message": head_commit.message,
                "content_hash": head_commit.content_hash.to_string(),
                "created_at": head_commit.created_at,
            }
        });
        if let Some(ref c) = raw_content {
            obj["content"] = serde_json::Value::String(c.clone());
        }
        output::print_json_ok(&obj);
    } else {
        output::print_kv_table(&[
            ("slug", prompt.slug.clone()),
            ("version", prompt.current_version.to_string()),
            ("commit", head_commit.id.to_string()),
            ("hash", head_commit.content_hash.to_string()),
            ("message", head_commit.message.clone().unwrap_or_else(|| "—".to_string())),
            ("updated", prompt.updated_at.format("%Y-%m-%d %H:%M UTC").to_string()),
        ]);
        if let Some(ref c) = raw_content {
            println!();
            println!("{c}");
        }
    }

    Ok(())
}
