use std::{
    fs,
    path::{Path, PathBuf},
};

use intr_core::ids::AccountId;

use crate::{
    error::{CliError, CliResult},
    ui::output,
};

/// Initialise a new Intentry space in the current directory.
pub fn run(slug: Option<&str>, json: bool) -> CliResult<()> {
    let cwd = std::env::current_dir()?;

    // Derive slug from argument or directory name.
    let space_slug = match slug {
        Some(s) => s.to_string(),
        None => cwd
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("my-space")
            .to_string(),
    };

    let intr_dir = cwd.join(".intr");

    if intr_dir.exists() {
        return Err(CliError::Generic(format!(
            "already an Intentry space (found {intr_dir:?})"
        )));
    }

    // Create directory layout.
    fs::create_dir_all(intr_dir.join("projections"))?;
    fs::create_dir_all(intr_dir.join("objects"))?;

    // HEAD file - points to the tip of the local event log.
    fs::write(intr_dir.join("HEAD"), "")?;

    // Empty event log.
    fs::write(intr_dir.join("events.jsonl"), "")?;

    // Persist the space slug.
    fs::write(intr_dir.join("SPACE"), &space_slug)?;

    // Persist a stable local owner ID (used before auth).
    let owner_id = AccountId::new();
    fs::write(intr_dir.join("OWNER_ID"), owner_id.to_string())?;

    // Write a .gitignore entry if a git repo is present.
    let gitignore = cwd.join(".gitignore");
    let intr_gitignore_entry = "\n# Intentry local state\n.intr/\n";
    if cwd.join(".git").exists() {
        let existing = fs::read_to_string(&gitignore).unwrap_or_default();
        if !existing.contains(".intr/") {
            let mut content = existing;
            content.push_str(intr_gitignore_entry);
            fs::write(&gitignore, content)?;
        }
    }

    if json {
        output::print_json_ok(&serde_json::json!({
            "slug": space_slug,
            "path": cwd.display().to_string(),
        }));
    } else {
        output::print_success(&format!(
            "Initialized Intentry space '{space_slug}' at {}",
            cwd.display()
        ));
    }

    Ok(())
}
