use std::fs;
use std::path::Path;

use intr_core::{
    store::{CommitInput, VersionStore},
    version::BumpKind,
};

use crate::{
    error::{CliError, CliResult},
    store::{local_owner_id, SpaceCtx},
    ui::output,
};

/// Commit all modified .prompt files in the current directory.
pub async fn run(
    message: Option<&str>,
    bump: Option<&str>,
    dry_run: bool,
    json: bool,
) -> CliResult<()> {
    let ctx = SpaceCtx::open().await?;
    let cwd = std::env::current_dir()?;
    let author_id = local_owner_id(&ctx.intr_dir);

    let bump_kind = parse_bump(bump)?;

    let prompt_files = walkdir_prompt_files(&cwd);
    if prompt_files.is_empty() {
        if json {
            output::print_json_ok(&serde_json::json!({ "committed": [] }));
        } else {
            output::print_info("no .prompt files found");
        }
        return Ok(());
    }

    let mut committed = Vec::new();
    let mut skipped = Vec::new();

    for path in &prompt_files {
        let slug = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        let raw_bytes = fs::read(path)
            .map_err(|e| CliError::Generic(format!("failed to read {}: {e}", path.display())))?;

        // Check if prompt exists in store.
        let existing = ctx.store.get_prompt_by_slug(&ctx.space.id, &slug).await;

        let is_new = existing.is_err();

        if !is_new {
            // Check if content has actually changed.
            let prompt = existing.as_ref().unwrap();
            if let Ok(commit) = ctx.store.get_commit(&prompt.head_commit_id).await {
                if let Ok(stored_bytes) = ctx.store.get_blob(&commit.content_hash).await {
                    if stored_bytes == raw_bytes {
                        skipped.push(slug.clone());
                        continue;
                    }
                }
            }
        }

        if dry_run {
            let verb = if is_new { "new" } else { "modified" };
            println!("  {verb}: {slug}");
            committed.push(slug);
            continue;
        }

        let input = CommitInput {
            space_id: ctx.space.id.clone(),
            author_id: author_id.clone(),
            prompt_id: existing.as_ref().ok().map(|p| p.id.clone()),
            slug: if is_new { Some(slug.clone()) } else { None },
            raw_bytes,
            message: message.map(|s| s.to_string()),
            bump: bump_kind.clone(),
        };

        let commit_result = if is_new {
            ctx.store.create_prompt(input).await
        } else {
            ctx.store.commit_prompt(input).await
        };

        let commit = commit_result.map_err(|e| CliError::Generic(e.to_string()))?;

        committed.push(format!("{slug}@v{}", commit.version));
    }

    if dry_run {
        output::print_info("(dry-run - no changes written)");
        return Ok(());
    }

    if json {
        output::print_json_ok(&serde_json::json!({
            "committed": committed,
            "skipped": skipped,
        }));
    } else if committed.is_empty() {
        output::print_success("nothing to commit - all prompts up to date");
    } else {
        output::print_success(&format!("committed {} prompt(s)", committed.len()));
        for item in &committed {
            println!("  + {item}");
        }
    }

    Ok(())
}

fn parse_bump(bump: Option<&str>) -> CliResult<BumpKind> {
    match bump {
        None | Some("patch") => Ok(BumpKind::Patch),
        Some("minor") => Ok(BumpKind::Minor),
        Some("major") => Ok(BumpKind::Major),
        Some(other) => Err(CliError::Usage(format!(
            "unknown bump kind '{other}' - expected patch, minor, or major"
        ))),
    }
}

fn walkdir_prompt_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.starts_with('.') {
                    results.extend(walkdir_prompt_files(&path));
                }
            } else if path.extension().and_then(|e| e.to_str()) == Some("prompt") {
                results.push(path);
            }
        }
    }
    results
}

