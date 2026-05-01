use intr_core::{
    store::{CommitInput, VersionStore},
    version::BumpKind,
};

use crate::{
    error::{CliError, CliResult},
    store::{local_owner_id, SpaceCtx},
    ui::output,
};

/// Scaffold a new .prompt file and register it in the store.
pub async fn run(slug: &str, tier: u8, no_commit: bool, json: bool) -> CliResult<()> {
    let ctx = SpaceCtx::open().await?;
    let cwd = std::env::current_dir()?;
    let file_path = cwd.join(format!("{slug}.prompt"));

    if file_path.exists() {
        return Err(CliError::Generic(format!(
            "file already exists: {}",
            file_path.display()
        )));
    }

    let content = scaffold_prompt(slug, tier);
    std::fs::write(&file_path, &content)?;

    if no_commit {
        if json {
            output::print_json_ok(&serde_json::json!({
                "slug": slug,
                "file": file_path.display().to_string(),
                "committed": false,
            }));
        } else {
            output::print_success(&format!(
                "Created {}.prompt (not committed - run `intr commit` to save)",
                slug
            ));
        }
        return Ok(());
    }

    let author_id = local_owner_id(&ctx.intr_dir);

    let commit = ctx
        .store
        .create_prompt(CommitInput {
            space_id: ctx.space.id.clone(),
            author_id,
            prompt_id: None,
            slug: Some(slug.to_string()),
            raw_bytes: content.into_bytes(),
            message: Some("initial version".to_string()),
            bump: BumpKind::Patch,
        })
        .await
        .map_err(|e| CliError::Generic(e.to_string()))?;

    if json {
        output::print_json_ok(&serde_json::json!({
            "slug": slug,
            "file": file_path.display().to_string(),
            "version": commit.version.to_string(),
            "commit_id": commit.id.to_string(),
        }));
    } else {
        output::print_success(&format!(
            "Created {}.prompt at v{}",
            slug, commit.version
        ));
    }

    Ok(())
}

fn scaffold_prompt(slug: &str, tier: u8) -> String {
    match tier {
        1 => format!("---\nid: {slug}\nversion: 0.1.0\n---\n\nYour prompt here.\n"),
        2 => format!(
            "---\nid: {slug}\nversion: 0.1.0\ndescription: \"\"\nmodel:\n  name: gpt-4o\n---\n\nYour prompt here.\n"
        ),
        3 => format!(
            "---\nid: {slug}\nversion: 0.1.0\ndescription: \"\"\nmodel:\n  name: gpt-4o\n  temperature: 0.7\nevals: []\nvariables: []\n---\n\nYour prompt here.\n"
        ),
        _ => format!("---\nid: {slug}\nversion: 0.1.0\n---\n\nYour prompt here.\n"),
    }
}

