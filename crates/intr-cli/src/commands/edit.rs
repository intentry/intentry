use std::{env, fs};

use intr_core::store::{CommitInput, VersionStore};
use intr_core::version::BumpKind;

use crate::{
    error::{CliError, CliResult},
    store::{local_owner_id, SpaceCtx},
    ui::output,
};

/// Open a prompt in $EDITOR and commit on save if content changed.
pub async fn run(prompt_slug: &str, json: bool) -> CliResult<()> {
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

    let original_bytes = ctx
        .store
        .get_blob(&head_commit.content_hash)
        .await
        .map_err(|e| CliError::Generic(e.to_string()))?;

    // Write content to a temp file.
    let tmp_path = {
        let tmp_dir = env::temp_dir();
        tmp_dir.join(format!("{prompt_slug}.prompt"))
    };
    fs::write(&tmp_path, &original_bytes)?;

    // Launch $EDITOR (fallback: vi on Unix, notepad on Windows).
    let editor = env::var("EDITOR")
        .or_else(|_| env::var("VISUAL"))
        .unwrap_or_else(|_| {
            if cfg!(target_os = "windows") {
                "notepad".to_string()
            } else {
                "vi".to_string()
            }
        });

    let status = std::process::Command::new(&editor)
        .arg(&tmp_path)
        .status()
        .map_err(|e| CliError::Generic(format!("failed to launch editor '{editor}': {e}")))?;

    if !status.success() {
        return Err(CliError::Generic(format!(
            "editor exited with status {}",
            status
        )));
    }

    let new_bytes = fs::read(&tmp_path)?;
    let _ = fs::remove_file(&tmp_path);

    if new_bytes == original_bytes {
        if json {
            output::print_json_ok(&serde_json::json!({ "changed": false }));
        } else {
            output::print_info("no changes - nothing committed");
        }
        return Ok(());
    }

    let author_id = local_owner_id(&ctx.intr_dir);

    let commit = ctx
        .store
        .commit_prompt(CommitInput {
            space_id: ctx.space.id.clone(),
            author_id,
            prompt_id: Some(prompt.id.clone()),
            slug: None,
            raw_bytes: new_bytes,
            message: None,
            bump: BumpKind::Patch,
        })
        .await
        .map_err(|e| CliError::Generic(e.to_string()))?;

    if json {
        output::print_json_ok(&serde_json::json!({
            "slug": prompt_slug,
            "version": commit.version.to_string(),
            "commit_id": commit.id.to_string(),
        }));
    } else {
        output::print_success(&format!(
            "Committed {prompt_slug} at v{}",
            commit.version
        ));
    }

    Ok(())
}

