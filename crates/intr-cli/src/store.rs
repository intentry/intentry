//! Space context — find the `.intr/` directory and open a `LocalStore`.
//!
//! Every command that touches the store calls [`SpaceCtx::open()`] first.
//! It walks up from the current directory looking for `.intr/`, then opens
//! (or migrates) the SQLite store inside it.

use std::path::{Path, PathBuf};

use intr_core::{
    ids::AccountId,
    local::LocalStore,
    store::{CreateSpaceInput, VersionStore},
    types::Space,
};

use crate::error::{CliError, CliResult};

/// A resolved space context: store + active space.
pub struct SpaceCtx {
    pub store: LocalStore,
    pub space: Space,
    /// The `.intr/` directory that was found.
    pub intr_dir: PathBuf,
}

impl SpaceCtx {
    /// Walk up from `cwd`, find `.intr/`, open the store, resolve the active space.
    ///
    /// Fails with a friendly hint if no `.intr/` directory is found.
    pub async fn open() -> CliResult<Self> {
        let cwd = std::env::current_dir()?;
        Self::open_from(&cwd).await
    }

    pub async fn open_from(start: &Path) -> CliResult<Self> {
        let intr_dir = find_intr_dir(start).ok_or_else(|| {
            CliError::Generic(
                "not an Intentry space (no .intr/ found — run `intr init` first)".into(),
            )
        })?;

        let store = LocalStore::open(&intr_dir)
            .await
            .map_err(|e| CliError::Generic(e.to_string()))?;

        // Resolve or bootstrap the space for this directory.
        let space = resolve_space(&store, &intr_dir).await?;

        Ok(Self {
            store,
            space,
            intr_dir,
        })
    }
}

/// Walk up the directory tree looking for `.intr/`.
pub fn find_intr_dir(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let candidate = current.join(".intr");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Resolve the active `Space` for this `.intr/` directory.
///
/// Strategy:
/// 1. Read the slug from `.intr/SPACE` (written by `intr init`).
/// 2. If found, look it up in the store (create if missing).
/// 3. If not found, derive slug from directory name and create.
async fn resolve_space(store: &LocalStore, intr_dir: &Path) -> CliResult<Space> {
    let slug = read_space_file(intr_dir).unwrap_or_else(|| {
        intr_dir
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("default")
            .to_string()
    });

    // Use a stable local owner ID — pre-auth local context.
    let owner_id = local_owner_id(intr_dir);

    // Try to fetch existing space.
    if let Ok(space) = store.get_space_by_slug(&owner_id, &slug).await {
        return Ok(space);
    }

    // Bootstrap a new space.
    let space = store
        .create_space(CreateSpaceInput {
            owner_id,
            slug: slug.clone(),
            description: None,
            is_public: false,
        })
        .await
        .map_err(|e| CliError::Generic(format!("failed to create space: {e}")))?;

    // Persist the slug for next time.
    write_space_file(intr_dir, &slug);

    Ok(space)
}

fn read_space_file(intr_dir: &Path) -> Option<String> {
    let path = intr_dir.join("SPACE");
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn write_space_file(intr_dir: &Path, slug: &str) {
    let _ = std::fs::write(intr_dir.join("SPACE"), slug);
}

/// A stable local owner ID read from `.intr/OWNER_ID`, persisted by `intr init`.
///
/// Falls back to generating a new ID if the file is missing (shouldn't happen
/// in normal use — `intr init` always writes this file).
pub fn local_owner_id(intr_dir: &Path) -> AccountId {
    let path = intr_dir.join("OWNER_ID");
    if let Ok(s) = std::fs::read_to_string(&path) {
        let s = s.trim();
        if !s.is_empty() {
            if let Ok(id) = s.parse() {
                return id;
            }
        }
    }
    // Generate and persist for next time.
    let id = AccountId::new();
    let _ = std::fs::write(&path, id.to_string());
    id
}
