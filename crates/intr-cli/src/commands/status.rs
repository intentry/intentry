use std::{fs, path::Path};

use intr_core::store::VersionStore;
use sha2::{Digest, Sha256};

use crate::{
    error::CliResult,
    store::SpaceCtx,
    ui::output,
};

#[derive(Debug, serde::Serialize)]
struct PromptStatus {
    slug: String,
    state: &'static str,
    file: String,
    version: Option<String>,
}

/// Show which .prompt files are modified, new, or unchanged vs the store.
pub async fn run(json: bool) -> CliResult<()> {
    let ctx = SpaceCtx::open().await?;
    let cwd = std::env::current_dir()?;

    let mut statuses: Vec<PromptStatus> = Vec::new();

    // Walk cwd for *.prompt files.
    for entry in walkdir_prompt_files(&cwd) {
        let path = entry;
        let slug = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        let file_bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };

        let file_hash = sha256_hex(&file_bytes);

        let rel = path
            .strip_prefix(&cwd)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();

        match ctx.store.get_prompt_by_slug(&ctx.space.id, &slug).await {
            Ok(prompt) => {
                // Compare content hash of head commit with current file.
                if let Ok(commit) = ctx.store.get_commit(&prompt.head_commit_id).await {
                    if commit.content_hash.to_string() == file_hash {
                        statuses.push(PromptStatus {
                            slug: slug.clone(),
                            state: "unchanged",
                            file: rel,
                            version: Some(prompt.current_version.to_string()),
                        });
                    } else {
                        statuses.push(PromptStatus {
                            slug: slug.clone(),
                            state: "modified",
                            file: rel,
                            version: Some(prompt.current_version.to_string()),
                        });
                    }
                }
            }
            Err(_) => {
                // Not tracked in store yet.
                statuses.push(PromptStatus {
                    slug: slug.clone(),
                    state: "new",
                    file: rel,
                    version: None,
                });
            }
        }
    }

    let modified: Vec<_> = statuses.iter().filter(|s| s.state == "modified").collect();
    let new_files: Vec<_> = statuses.iter().filter(|s| s.state == "new").collect();
    let unchanged: Vec<_> = statuses.iter().filter(|s| s.state == "unchanged").collect();

    if json {
        output::print_json_ok(&statuses);
    } else {
        if modified.is_empty() && new_files.is_empty() {
            if unchanged.is_empty() {
                output::print_info("no .prompt files found");
            } else {
                output::print_success("everything up to date");
            }
            return Ok(());
        }

        if !new_files.is_empty() {
            println!("new (untracked):");
            for s in &new_files {
                println!("  + {}", s.file);
            }
        }
        if !modified.is_empty() {
            println!("modified:");
            for s in &modified {
                println!(
                    "  ~ {}  (v{})",
                    s.file,
                    s.version.as_deref().unwrap_or("?")
                );
            }
        }
        println!();
        println!("Run `intr commit` to save changes.");
    }

    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn walkdir_prompt_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut results = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Recurse, but skip hidden dirs and .intr.
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

