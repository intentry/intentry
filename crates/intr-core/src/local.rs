/// Local SQLite-backed store for offline CLI use.
///
/// `LocalStore` implements [`crate::store::VersionStore`] using:
/// - **SQLite** (via `sqlx`) for structured data (spaces, prompts, commits, events).
/// - **Filesystem** for content-addressed blob storage.
///
/// Data layout under `data_dir`:
///
/// ```text
/// <data_dir>/
///   store.sqlite               - SQLite database
///   objects/
///     sha256/
///       <first2>/
///         <rest>               - raw .prompt bytes
/// ```
///
/// The same `data_dir` can be `~/.intr/` for a shared user store or a
/// per-project `.intr/` directory for workspace-scoped spaces.
use std::path::{Path, PathBuf};

use chrono::Utc;
use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    SqlitePool,
};

use crate::{
    error::{StorageError, VersionStoreError},
    events::{Event, EventCursor, EventPayload},
    ids::{AccountId, CommitId, ContentHash, PromptId, SpaceId},
    store::{
        CommitInput, CreateSpaceInput, ForkInput, Page, PageRequest, PromptFilter,
        VersionStore,
    },
    types::{Commit, Prompt, Space},
    version::{BumpKind, SemVer},
};

// ---------------------------------------------------------------------------
// SQL schema
// ---------------------------------------------------------------------------

const SCHEMA_STMTS: &[&str] = &[
    "PRAGMA journal_mode = WAL",
    "PRAGMA foreign_keys = ON",
    r#"CREATE TABLE IF NOT EXISTS spaces (
        id          TEXT PRIMARY KEY,
        owner_id    TEXT NOT NULL,
        slug        TEXT NOT NULL,
        description TEXT,
        is_public   INTEGER NOT NULL DEFAULT 1,
        created_at  TEXT NOT NULL,
        updated_at  TEXT NOT NULL,
        UNIQUE(owner_id, slug)
    )"#,
    r#"CREATE TABLE IF NOT EXISTS prompts (
        id               TEXT PRIMARY KEY,
        space_id         TEXT NOT NULL REFERENCES spaces(id),
        slug             TEXT NOT NULL,
        head_commit_id   TEXT NOT NULL,
        current_version  TEXT NOT NULL,
        created_at       TEXT NOT NULL,
        updated_at       TEXT NOT NULL,
        UNIQUE(space_id, slug)
    )"#,
    r#"CREATE TABLE IF NOT EXISTS commits (
        id            TEXT PRIMARY KEY,
        prompt_id     TEXT NOT NULL REFERENCES prompts(id),
        space_id      TEXT NOT NULL REFERENCES spaces(id),
        author_id     TEXT NOT NULL,
        content_hash  TEXT NOT NULL,
        version       TEXT NOT NULL,
        message       TEXT,
        parent_id     TEXT,
        created_at    TEXT NOT NULL,
        UNIQUE(prompt_id, version)
    )"#,
    r#"CREATE TABLE IF NOT EXISTS events (
        seq          INTEGER PRIMARY KEY AUTOINCREMENT,
        occurred_at  TEXT NOT NULL,
        actor_id     TEXT NOT NULL,
        space_id     TEXT NOT NULL,
        payload      TEXT NOT NULL
    )"#,
    "CREATE INDEX IF NOT EXISTS idx_prompts_space ON prompts(space_id)",
    "CREATE INDEX IF NOT EXISTS idx_commits_prompt ON commits(prompt_id, created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_events_space_seq ON events(space_id, seq)",
];

// ---------------------------------------------------------------------------
// Row types (for sqlx::query_as)
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct SpaceRow {
    id: String,
    owner_id: String,
    slug: String,
    description: Option<String>,
    is_public: i64,
    created_at: String,
    updated_at: String,
}

#[derive(sqlx::FromRow)]
struct PromptRow {
    id: String,
    space_id: String,
    slug: String,
    head_commit_id: String,
    current_version: String,
    created_at: String,
    updated_at: String,
}

#[derive(sqlx::FromRow)]
struct CommitRow {
    id: String,
    prompt_id: String,
    space_id: String,
    author_id: String,
    content_hash: String,
    version: String,
    message: Option<String>,
    parent_id: Option<String>,
    created_at: String,
}

// ---------------------------------------------------------------------------
// Row → domain conversions
// ---------------------------------------------------------------------------

fn space_from_row(r: SpaceRow) -> Result<Space, StorageError> {
    Ok(Space {
        id: r.id.parse().map_err(|e: crate::ids::IdParseError| {
            StorageError::Serialization(e.to_string())
        })?,
        owner_id: r.owner_id.parse().map_err(|e: crate::ids::IdParseError| {
            StorageError::Serialization(e.to_string())
        })?,
        slug: r.slug,
        description: r.description,
        is_public: r.is_public != 0,
        created_at: parse_dt(&r.created_at)?,
        updated_at: parse_dt(&r.updated_at)?,
    })
}

fn prompt_from_row(r: PromptRow) -> Result<Prompt, StorageError> {
    let current_version = semver::Version::parse(&r.current_version)
        .map_err(|e| StorageError::Serialization(e.to_string()))?;
    Ok(Prompt {
        id: r.id.parse().map_err(|e: crate::ids::IdParseError| {
            StorageError::Serialization(e.to_string())
        })?,
        space_id: r.space_id.parse().map_err(|e: crate::ids::IdParseError| {
            StorageError::Serialization(e.to_string())
        })?,
        slug: r.slug,
        head_commit_id: r.head_commit_id.parse().map_err(|e: crate::ids::IdParseError| {
            StorageError::Serialization(e.to_string())
        })?,
        current_version,
        created_at: parse_dt(&r.created_at)?,
        updated_at: parse_dt(&r.updated_at)?,
    })
}

fn commit_from_row(r: CommitRow) -> Result<Commit, StorageError> {
    let version = semver::Version::parse(&r.version)
        .map_err(|e| StorageError::Serialization(e.to_string()))?;
    let content_hash: ContentHash = r.content_hash.parse().map_err(|e: crate::ids::ContentHashParseError| {
        StorageError::Serialization(e.to_string())
    })?;
    let parent_id = r
        .parent_id
        .map(|s| s.parse::<CommitId>())
        .transpose()
        .map_err(|e: crate::ids::IdParseError| StorageError::Serialization(e.to_string()))?;
    Ok(Commit {
        id: r.id.parse().map_err(|e: crate::ids::IdParseError| {
            StorageError::Serialization(e.to_string())
        })?,
        prompt_id: r.prompt_id.parse().map_err(|e: crate::ids::IdParseError| {
            StorageError::Serialization(e.to_string())
        })?,
        space_id: r.space_id.parse().map_err(|e: crate::ids::IdParseError| {
            StorageError::Serialization(e.to_string())
        })?,
        author_id: r.author_id.parse().map_err(|e: crate::ids::IdParseError| {
            StorageError::Serialization(e.to_string())
        })?,
        content_hash,
        version,
        message: r.message,
        parent_id,
        created_at: parse_dt(&r.created_at)?,
    })
}

fn parse_dt(s: &str) -> Result<chrono::DateTime<Utc>, StorageError> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| StorageError::Serialization(e.to_string()))
}

// ---------------------------------------------------------------------------
// LocalStore
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct LocalStore {
    pool: SqlitePool,
    data_dir: PathBuf,
}

impl LocalStore {
    /// Open or create a `LocalStore` rooted at `data_dir`.
    ///
    /// Creates the directory structure if it does not exist. Safe to call
    /// concurrently - SQLite WAL mode handles readers/writers.
    pub async fn open(data_dir: &Path) -> Result<Self, StorageError> {
        tokio::fs::create_dir_all(data_dir)
            .await
            .map_err(StorageError::Io)?;

        let db_path = data_dir.join("store.sqlite");
        let opts = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);

        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .map_err(|e| StorageError::Sqlite(e.to_string()))?;

        Self::run_migrations(&pool).await?;

        Ok(Self {
            pool,
            data_dir: data_dir.to_path_buf(),
        })
    }

    async fn run_migrations(pool: &SqlitePool) -> Result<(), StorageError> {
        for stmt in SCHEMA_STMTS {
            sqlx::query(stmt)
                .execute(pool)
                .await
                .map_err(|e| StorageError::Sqlite(e.to_string()))?;
        }
        Ok(())
    }

    // -- Blob store ---------------------------------------------------------

    fn blob_path(&self, hash: &ContentHash) -> PathBuf {
        let hex = hash.hex();
        let (prefix, rest) = hex.split_at(2);
        self.data_dir
            .join("objects")
            .join("sha256")
            .join(prefix)
            .join(rest)
    }

    async fn write_blob_bytes(
        &self,
        hash: &ContentHash,
        bytes: &[u8],
    ) -> Result<(), StorageError> {
        let path = self.blob_path(hash);
        if path.exists() {
            return Ok(()); // content-addressed: already present
        }
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(StorageError::Io)?;
        }
        tokio::fs::write(&path, bytes)
            .await
            .map_err(StorageError::Io)?;
        Ok(())
    }

    async fn read_blob_bytes(&self, hash: &ContentHash) -> Result<Vec<u8>, StorageError> {
        let path = self.blob_path(hash);
        tokio::fs::read(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::Sqlite(format!("blob not found: {}", hash))
            } else {
                StorageError::Io(e)
            }
        })
    }

    // -- Event append -------------------------------------------------------

    async fn append_event(
        &self,
        actor_id: &AccountId,
        space_id: &SpaceId,
        payload: &EventPayload,
    ) -> Result<u64, StorageError> {
        let payload_json =
            serde_json::to_string(payload).map_err(|e| StorageError::Serialization(e.to_string()))?;
        let now = Utc::now().to_rfc3339();
        let actor_str = actor_id.to_string();
        let space_str = space_id.to_string();

        let result = sqlx::query(
            "INSERT INTO events (occurred_at, actor_id, space_id, payload) VALUES (?, ?, ?, ?)",
        )
        .bind(&now)
        .bind(&actor_str)
        .bind(&space_str)
        .bind(&payload_json)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Sqlite(e.to_string()))?;

        Ok(result.last_insert_rowid() as u64)
    }

    // -- Version resolution -------------------------------------------------

    fn resolve_new_version(
        &self,
        parsed: &intr_parser::ParseResult,
        current: Option<&semver::Version>,
        bump: BumpKind,
    ) -> Result<semver::Version, VersionStoreError> {
        match bump {
            BumpKind::Explicit => {
                let ver_str = parsed
                    .frontmatter
                    .as_ref()
                    .and_then(|fm| fm.version.as_deref())
                    .ok_or_else(|| {
                        VersionStoreError::Validation(
                            "BumpKind::Explicit requires a `version:` field in frontmatter".into(),
                        )
                    })?;
                semver::Version::parse(ver_str)
                    .map_err(|e| VersionStoreError::Validation(e.to_string()))
            }
            bump_kind => {
                let base = current.cloned().unwrap_or(semver::Version::new(1, 0, 0));
                if current.is_none() {
                    // First commit - always 1.0.0 unless bumped
                    return Ok(base);
                }
                let sv = SemVer(base);
                Ok(match bump_kind {
                    BumpKind::Patch => sv.bump_patch(),
                    BumpKind::Minor => sv.bump_minor(),
                    BumpKind::Major => sv.bump_major(),
                    BumpKind::Explicit => unreachable!(),
                }
                .0)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// VersionStore implementation
// ---------------------------------------------------------------------------

impl VersionStore for LocalStore {
    // -- Spaces -------------------------------------------------------------

    async fn create_space(
        &self,
        input: CreateSpaceInput,
    ) -> Result<Space, VersionStoreError> {
        let id = SpaceId::new();
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        sqlx::query(
            "INSERT INTO spaces (id, owner_id, slug, description, is_public, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(input.owner_id.to_string())
        .bind(&input.slug)
        .bind(&input.description)
        .bind(if input.is_public { 1i64 } else { 0i64 })
        .bind(&now_str)
        .bind(&now_str)
        .execute(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db) if db.is_unique_violation() => {
                VersionStoreError::Conflict(format!("space '{}' already exists", input.slug))
            }
            e => VersionStoreError::Storage(StorageError::Sqlite(e.to_string())),
        })?;

        let space = Space {
            id,
            owner_id: input.owner_id,
            slug: input.slug,
            description: input.description,
            is_public: input.is_public,
            created_at: now,
            updated_at: now,
        };

        self.append_event(
            &space.owner_id,
            &space.id,
            &EventPayload::SpaceCreated {
                space_id: space.id.clone(),
                slug: space.slug.clone(),
                is_public: space.is_public,
            },
        )
        .await
        .map_err(VersionStoreError::Storage)?;

        Ok(space)
    }

    async fn get_space(&self, id: &SpaceId) -> Result<Space, VersionStoreError> {
        let row = sqlx::query_as::<_, SpaceRow>("SELECT * FROM spaces WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| VersionStoreError::Storage(StorageError::Sqlite(e.to_string())))?
            .ok_or_else(|| VersionStoreError::NotFound(format!("space {id}")))?;
        space_from_row(row).map_err(VersionStoreError::Storage)
    }

    async fn get_space_by_slug(
        &self,
        owner_id: &AccountId,
        slug: &str,
    ) -> Result<Space, VersionStoreError> {
        let row = sqlx::query_as::<_, SpaceRow>(
            "SELECT * FROM spaces WHERE owner_id = ? AND slug = ?",
        )
        .bind(owner_id.to_string())
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| VersionStoreError::Storage(StorageError::Sqlite(e.to_string())))?
        .ok_or_else(|| VersionStoreError::NotFound(format!("space '{slug}'")))?;
        space_from_row(row).map_err(VersionStoreError::Storage)
    }

    // -- Prompts ------------------------------------------------------------

    async fn create_prompt(
        &self,
        input: CommitInput,
    ) -> Result<Commit, VersionStoreError> {
        let slug = input.slug.as_deref().ok_or_else(|| {
            VersionStoreError::Validation("slug is required when creating a new prompt".into())
        })?;

        let parsed = intr_parser::parse(&input.raw_bytes)
            .map_err(|e| VersionStoreError::Validation(e.to_string()))?;

        let version = self.resolve_new_version(&parsed, None, input.bump)?;
        let content_hash = ContentHash::of(&input.raw_bytes);

        self.write_blob_bytes(&content_hash, &input.raw_bytes)
            .await
            .map_err(VersionStoreError::Storage)?;

        let prompt_id = PromptId::new();
        let commit_id = CommitId::new();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let ver_str = version.to_string();
        let hash_str = content_hash.to_string();

        // Insert commit + prompt atomically.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| VersionStoreError::Storage(StorageError::Sqlite(e.to_string())))?;

        // Insert prompt first (FK: commits.prompt_id → prompts.id).
        sqlx::query(
            "INSERT INTO prompts (id, space_id, slug, head_commit_id, current_version, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(prompt_id.to_string())
        .bind(input.space_id.to_string())
        .bind(slug)
        .bind(commit_id.to_string())
        .bind(&ver_str)
        .bind(&now_str)
        .bind(&now_str)
        .execute(&mut *tx)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db) if db.is_unique_violation() => {
                VersionStoreError::Conflict(format!(
                    "prompt '{}' already exists in this space",
                    slug
                ))
            }
            e => VersionStoreError::Storage(StorageError::Sqlite(e.to_string())),
        })?;

        sqlx::query(
            "INSERT INTO commits (id, prompt_id, space_id, author_id, content_hash, version, message, parent_id, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, NULL, ?)",
        )
        .bind(commit_id.to_string())
        .bind(prompt_id.to_string())
        .bind(input.space_id.to_string())
        .bind(input.author_id.to_string())
        .bind(&hash_str)
        .bind(&ver_str)
        .bind(&input.message)
        .bind(&now_str)
        .execute(&mut *tx)
        .await
        .map_err(|e| VersionStoreError::Storage(StorageError::Sqlite(e.to_string())))?;

        tx.commit()
            .await
            .map_err(|e| VersionStoreError::Storage(StorageError::Sqlite(e.to_string())))?;

        self.append_event(
            &input.author_id,
            &input.space_id,
            &EventPayload::PromptCreated {
                prompt_id: prompt_id.clone(),
                space_id: input.space_id.clone(),
                slug: slug.to_string(),
                commit_id: commit_id.clone(),
                content_hash: content_hash.clone(),
                version: version.clone(),
                message: input.message.clone(),
            },
        )
        .await
        .map_err(VersionStoreError::Storage)?;

        Ok(Commit {
            id: commit_id,
            prompt_id,
            space_id: input.space_id,
            author_id: input.author_id,
            content_hash,
            version,
            message: input.message,
            parent_id: None,
            created_at: now,
        })
    }

    async fn commit_prompt(
        &self,
        input: CommitInput,
    ) -> Result<Commit, VersionStoreError> {
        let prompt_id = input.prompt_id.as_ref().ok_or_else(|| {
            VersionStoreError::Validation("prompt_id is required when committing to an existing prompt".into())
        })?;

        // Load current prompt to get head version + head commit ID.
        let prompt = self.get_prompt(prompt_id).await?;

        let parsed = intr_parser::parse(&input.raw_bytes)
            .map_err(|e| VersionStoreError::Validation(e.to_string()))?;

        let new_version =
            self.resolve_new_version(&parsed, Some(&prompt.current_version), input.bump)?;

        // Validate version is strictly greater than current.
        if new_version <= prompt.current_version {
            return Err(VersionStoreError::Conflict(format!(
                "new version {} must be greater than current version {}",
                new_version, prompt.current_version
            )));
        }

        let content_hash = ContentHash::of(&input.raw_bytes);

        // Idempotency: if this exact content is already the head, return the head commit.
        let head = self.get_commit(&prompt.head_commit_id).await?;
        if content_hash == head.content_hash {
            return Ok(head);
        }

        self.write_blob_bytes(&content_hash, &input.raw_bytes)
            .await
            .map_err(VersionStoreError::Storage)?;

        let commit_id = CommitId::new();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let ver_str = new_version.to_string();
        let hash_str = content_hash.to_string();
        let parent_commit_id = prompt.head_commit_id.clone();
        let parent_str = parent_commit_id.to_string();

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| VersionStoreError::Storage(StorageError::Sqlite(e.to_string())))?;

        sqlx::query(
            "INSERT INTO commits (id, prompt_id, space_id, author_id, content_hash, version, message, parent_id, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(commit_id.to_string())
        .bind(prompt_id.to_string())
        .bind(input.space_id.to_string())
        .bind(input.author_id.to_string())
        .bind(&hash_str)
        .bind(&ver_str)
        .bind(&input.message)
        .bind(&parent_str)
        .bind(&now_str)
        .execute(&mut *tx)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db) if db.is_unique_violation() => {
                VersionStoreError::Conflict(format!(
                    "version {} already exists on this prompt",
                    ver_str
                ))
            }
            e => VersionStoreError::Storage(StorageError::Sqlite(e.to_string())),
        })?;

        sqlx::query(
            "UPDATE prompts SET head_commit_id = ?, current_version = ?, updated_at = ? WHERE id = ?",
        )
        .bind(commit_id.to_string())
        .bind(&ver_str)
        .bind(&now_str)
        .bind(prompt_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| VersionStoreError::Storage(StorageError::Sqlite(e.to_string())))?;

        tx.commit()
            .await
            .map_err(|e| VersionStoreError::Storage(StorageError::Sqlite(e.to_string())))?;

        self.append_event(
            &input.author_id,
            &input.space_id,
            &EventPayload::PromptCommitted {
                prompt_id: prompt_id.clone(),
                space_id: input.space_id.clone(),
                commit_id: commit_id.clone(),
                parent_commit_id: parent_commit_id.clone(),
                content_hash: content_hash.clone(),
                version: new_version.clone(),
                message: input.message.clone(),
            },
        )
        .await
        .map_err(VersionStoreError::Storage)?;

        Ok(Commit {
            id: commit_id,
            prompt_id: prompt_id.clone(),
            space_id: input.space_id,
            author_id: input.author_id,
            content_hash,
            version: new_version,
            message: input.message,
            parent_id: Some(parent_commit_id),
            created_at: now,
        })
    }

    async fn fork_prompt(&self, input: ForkInput) -> Result<Commit, VersionStoreError> {
        let source_commit = self.get_commit(&input.source_commit_id).await?;
        let source_blob = self.get_blob(&source_commit.content_hash).await?;

        // Inject fork attribution into frontmatter.
        let forked_bytes = inject_fork_attribution(
            &source_blob,
            &source_commit,
        )
        .map_err(|e| VersionStoreError::Validation(e.to_string()))?;

        let commit_input = CommitInput {
            space_id: input.target_space_id.clone(),
            author_id: input.author_id.clone(),
            prompt_id: None,
            slug: Some(input.new_slug),
            raw_bytes: forked_bytes,
            message: Some("forked".to_string()),
            bump: BumpKind::Explicit, // forks always start at 1.0.0 or frontmatter version
        };

        // Re-use create_prompt; the injected frontmatter has version = 1.0.0.
        // If BumpKind::Explicit finds no frontmatter version, fall back to Patch (1.0.0 first).
        let result = self.create_prompt(commit_input.clone()).await;

        match result {
            Ok(commit) => {
                // Emit a PromptForked event (in addition to PromptCreated already emitted).
                self.append_event(
                    &input.author_id,
                    &input.target_space_id,
                    &EventPayload::PromptForked {
                        prompt_id: commit.prompt_id.clone(),
                        space_id: input.target_space_id.clone(),
                        slug: commit_input.slug.unwrap_or_default(),
                        commit_id: commit.id.clone(),
                        content_hash: commit.content_hash.clone(),
                        version: commit.version.clone(),
                        parent_prompt_id: source_commit.prompt_id.clone(),
                        parent_space_id: source_commit.space_id.clone(),
                        parent_version: source_commit.version.clone(),
                    },
                )
                .await
                .map_err(VersionStoreError::Storage)?;
                Ok(commit)
            }
            // If BumpKind::Explicit fails (no version in injected frontmatter),
            // fall back to Patch (starts at 1.0.0).
            Err(VersionStoreError::Validation(_)) => {
                let fallback = CommitInput {
                    bump: BumpKind::Patch,
                    ..commit_input
                };
                self.create_prompt(fallback).await
            }
            Err(e) => Err(e),
        }
    }

    async fn get_prompt(&self, id: &PromptId) -> Result<Prompt, VersionStoreError> {
        let row =
            sqlx::query_as::<_, PromptRow>("SELECT * FROM prompts WHERE id = ?")
                .bind(id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| VersionStoreError::Storage(StorageError::Sqlite(e.to_string())))?
                .ok_or_else(|| VersionStoreError::NotFound(format!("prompt {id}")))?;
        prompt_from_row(row).map_err(VersionStoreError::Storage)
    }

    async fn get_prompt_by_slug(
        &self,
        space_id: &SpaceId,
        slug: &str,
    ) -> Result<Prompt, VersionStoreError> {
        let row = sqlx::query_as::<_, PromptRow>(
            "SELECT * FROM prompts WHERE space_id = ? AND slug = ?",
        )
        .bind(space_id.to_string())
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| VersionStoreError::Storage(StorageError::Sqlite(e.to_string())))?
        .ok_or_else(|| VersionStoreError::NotFound(format!("prompt '{slug}'")))?;
        prompt_from_row(row).map_err(VersionStoreError::Storage)
    }

    async fn list_prompts(
        &self,
        space_id: &SpaceId,
        filter: PromptFilter,
        page: PageRequest,
    ) -> Result<Page<Prompt>, VersionStoreError> {
        let limit = page.limit.min(100) as i64;
        let after_id = page.cursor.as_deref().unwrap_or("");

        // Build query with optional search filter.
        let rows: Vec<PromptRow> = if let Some(ref q) = filter.query {
            let pattern = format!("%{}%", q);
            sqlx::query_as::<_, PromptRow>(
                "SELECT * FROM prompts WHERE space_id = ? AND slug LIKE ? AND id > ?
                 ORDER BY id LIMIT ?",
            )
            .bind(space_id.to_string())
            .bind(&pattern)
            .bind(after_id)
            .bind(limit + 1)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as::<_, PromptRow>(
                "SELECT * FROM prompts WHERE space_id = ? AND id > ? ORDER BY id LIMIT ?",
            )
            .bind(space_id.to_string())
            .bind(after_id)
            .bind(limit + 1)
            .fetch_all(&self.pool)
            .await
        }
        .map_err(|e| VersionStoreError::Storage(StorageError::Sqlite(e.to_string())))?;

        let has_more = rows.len() as i64 > limit;
        let items: Vec<Prompt> = rows
            .into_iter()
            .take(limit as usize)
            .map(prompt_from_row)
            .collect::<Result<_, _>>()
            .map_err(VersionStoreError::Storage)?;

        let next_cursor = if has_more {
            items.last().map(|p| p.id.to_string())
        } else {
            None
        };

        Ok(Page {
            items,
            next_cursor,
            total_count: None,
        })
    }

    // -- Commits ------------------------------------------------------------

    async fn get_commit(&self, id: &CommitId) -> Result<Commit, VersionStoreError> {
        let row =
            sqlx::query_as::<_, CommitRow>("SELECT * FROM commits WHERE id = ?")
                .bind(id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| VersionStoreError::Storage(StorageError::Sqlite(e.to_string())))?
                .ok_or_else(|| VersionStoreError::NotFound(format!("commit {id}")))?;
        commit_from_row(row).map_err(VersionStoreError::Storage)
    }

    async fn list_commits(
        &self,
        prompt_id: &PromptId,
        page: PageRequest,
    ) -> Result<Page<Commit>, VersionStoreError> {
        let limit = page.limit.min(100) as i64;
        let after_id = page.cursor.as_deref().unwrap_or("");

        let rows: Vec<CommitRow> = sqlx::query_as::<_, CommitRow>(
            "SELECT * FROM commits WHERE prompt_id = ? AND id > ?
             ORDER BY created_at DESC LIMIT ?",
        )
        .bind(prompt_id.to_string())
        .bind(after_id)
        .bind(limit + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| VersionStoreError::Storage(StorageError::Sqlite(e.to_string())))?;

        let has_more = rows.len() as i64 > limit;
        let items: Vec<Commit> = rows
            .into_iter()
            .take(limit as usize)
            .map(commit_from_row)
            .collect::<Result<_, _>>()
            .map_err(VersionStoreError::Storage)?;

        let next_cursor = if has_more {
            items.last().map(|c| c.id.to_string())
        } else {
            None
        };

        Ok(Page {
            items,
            next_cursor,
            total_count: None,
        })
    }

    // -- Blobs --------------------------------------------------------------

    async fn put_blob(&self, bytes: &[u8]) -> Result<ContentHash, VersionStoreError> {
        let hash = ContentHash::of(bytes);
        self.write_blob_bytes(&hash, bytes)
            .await
            .map_err(VersionStoreError::Storage)?;
        Ok(hash)
    }

    async fn get_blob(&self, hash: &ContentHash) -> Result<Vec<u8>, VersionStoreError> {
        self.read_blob_bytes(hash)
            .await
            .map_err(VersionStoreError::Storage)
    }

    // -- Events -------------------------------------------------------------

    async fn list_events(
        &self,
        space_id: &SpaceId,
        from: EventCursor,
        limit: u32,
    ) -> Result<Vec<Event>, VersionStoreError> {
        let limit = limit.min(500) as i64;
        let rows = sqlx::query(
            "SELECT seq, occurred_at, actor_id, space_id, payload
             FROM events WHERE space_id = ? AND seq > ?
             ORDER BY seq ASC LIMIT ?",
        )
        .bind(space_id.to_string())
        .bind(from.seq as i64)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| VersionStoreError::Storage(StorageError::Sqlite(e.to_string())))?;

        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            let seq: i64 = row.get("seq");
            let occurred_at_str: &str = row.get("occurred_at");
            let actor_id_str: &str = row.get("actor_id");
            let space_id_str: &str = row.get("space_id");
            let payload_str: &str = row.get("payload");

            let payload: EventPayload = serde_json::from_str(payload_str).map_err(|e| {
                VersionStoreError::Storage(StorageError::Serialization(e.to_string()))
            })?;

            events.push(Event {
                seq: seq as u64,
                occurred_at: parse_dt(occurred_at_str)
                    .map_err(VersionStoreError::Storage)?,
                actor_id: actor_id_str
                    .parse()
                    .map_err(|e: crate::ids::IdParseError| {
                        VersionStoreError::Storage(StorageError::Serialization(e.to_string()))
                    })?,
                space_id: space_id_str
                    .parse()
                    .map_err(|e: crate::ids::IdParseError| {
                        VersionStoreError::Storage(StorageError::Serialization(e.to_string()))
                    })?,
                payload,
            });
        }
        Ok(events)
    }
}

// ---------------------------------------------------------------------------
// Fork attribution injection
// ---------------------------------------------------------------------------

/// Inject `intentry.parent` and `intentry.forked_at` into a `.prompt` file.
///
/// If the source has YAML frontmatter, the `intentry` block is added/updated.
/// If the source is Tier 1 (no frontmatter), a minimal frontmatter block is
/// prepended with the intentry attribution and `version: "1.0.0"`.
fn inject_fork_attribution(
    source_bytes: &[u8],
    source_commit: &Commit,
) -> Result<Vec<u8>, String> {
    let src =
        std::str::from_utf8(source_bytes).map_err(|e| e.to_string())?;

    let parent_ref = format!(
        "{}@{}",
        source_commit.prompt_id,
        source_commit.version
    );
    let forked_at = Utc::now().to_rfc3339();

    // Split into optional YAML block + body.
    let (yaml_opt, body) = split_frontmatter_raw(src);

    let new_yaml = match yaml_opt {
        Some(yaml_str) => {
            let mut value: serde_yaml::Value = serde_yaml::from_str(yaml_str)
                .map_err(|e| e.to_string())?;
            let map = value
                .as_mapping_mut()
                .ok_or("frontmatter is not a YAML mapping")?;

            let intentry_key = serde_yaml::Value::String("intentry".into());
            let intentry = map
                .entry(intentry_key)
                .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));

            if let Some(im) = intentry.as_mapping_mut() {
                im.insert(
                    "parent".into(),
                    serde_yaml::Value::String(parent_ref),
                );
                im.insert(
                    "forked_at".into(),
                    serde_yaml::Value::String(forked_at),
                );
            }

            serde_yaml::to_string(&value).map_err(|e| e.to_string())?
        }
        None => {
            // No frontmatter - inject minimal block.
            format!(
                "version: \"1.0.0\"\nintentry:\n  parent: \"{}\"\n  forked_at: \"{}\"\n",
                parent_ref, forked_at
            )
        }
    };

    let new_content = format!("---\n{}---\n{}", new_yaml, body);
    Ok(new_content.into_bytes())
}

/// Split raw source into `(Some(yaml_str), body)` or `(None, full_src)`.
fn split_frontmatter_raw(src: &str) -> (Option<&str>, &str) {
    let src = src.trim_start();
    if !src.starts_with("---") {
        return (None, src);
    }
    let after_open = src[3..].trim_start_matches([' ', '\t', '\r', '\n']);
    // Find closing `---` at start of a line.
    let mut pos = 0;
    while pos < after_open.len() {
        let rest = &after_open[pos..];
        if (pos == 0 || after_open.as_bytes().get(pos - 1) == Some(&b'\n'))
            && rest.starts_with("---")
        {
            let yaml = &after_open[..pos];
            let body = after_open[pos + 3..].trim_start_matches(['\r', '\n']);
            return (Some(yaml), body);
        }
        pos += 1;
    }
    (None, src)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{CommitInput, CreateSpaceInput};
    use crate::version::BumpKind;

    async fn test_store() -> LocalStore {
        let dir = std::env::temp_dir()
            .join(format!("intr-test-{}", uuid::Uuid::now_v7()));
        LocalStore::open(&dir).await.expect("open LocalStore")
    }

    fn test_account() -> AccountId {
        AccountId::new()
    }

    #[tokio::test]
    async fn create_and_get_space() {
        let store = test_store().await;
        let owner = test_account();
        let space = store
            .create_space(CreateSpaceInput {
                owner_id: owner.clone(),
                slug: "my-space".into(),
                description: Some("test".into()),
                is_public: true,
            })
            .await
            .unwrap();

        assert_eq!(space.slug, "my-space");

        let fetched = store.get_space(&space.id).await.unwrap();
        assert_eq!(fetched.id, space.id);
        assert_eq!(fetched.slug, "my-space");

        let by_slug = store
            .get_space_by_slug(&owner, "my-space")
            .await
            .unwrap();
        assert_eq!(by_slug.id, space.id);
    }

    #[tokio::test]
    async fn duplicate_space_slug_is_conflict() {
        let store = test_store().await;
        let owner = test_account();
        store
            .create_space(CreateSpaceInput {
                owner_id: owner.clone(),
                slug: "dupe".into(),
                description: None,
                is_public: true,
            })
            .await
            .unwrap();
        let err = store
            .create_space(CreateSpaceInput {
                owner_id: owner,
                slug: "dupe".into(),
                description: None,
                is_public: true,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, VersionStoreError::Conflict(_)));
    }

    #[tokio::test]
    async fn create_prompt_and_commit() {
        let store = test_store().await;
        let owner = test_account();
        let space = store
            .create_space(CreateSpaceInput {
                owner_id: owner.clone(),
                slug: "s".into(),
                description: None,
                is_public: true,
            })
            .await
            .unwrap();

        let content = b"---\nid: hello\nversion: 1.0.0\ndescription: test\n---\nHello {{name}}!\n";
        let first = store
            .create_prompt(CommitInput {
                space_id: space.id.clone(),
                author_id: owner.clone(),
                prompt_id: None,
                slug: Some("hello".into()),
                raw_bytes: content.to_vec(),
                message: Some("init".into()),
                bump: BumpKind::Explicit,
            })
            .await
            .unwrap();

        assert_eq!(first.version.to_string(), "1.0.0");

        let prompt = store.get_prompt(&first.prompt_id).await.unwrap();
        assert_eq!(prompt.slug, "hello");
        assert_eq!(prompt.head_commit_id, first.id);

        // Second commit - patch bump.
        let content2 =
            b"---\nid: hello\nversion: 1.0.1\ndescription: test v2\n---\nHello, {{name}}!\n";
        let second = store
            .commit_prompt(CommitInput {
                space_id: space.id.clone(),
                author_id: owner.clone(),
                prompt_id: Some(first.prompt_id.clone()),
                slug: None,
                raw_bytes: content2.to_vec(),
                message: Some("patch".into()),
                bump: BumpKind::Explicit,
            })
            .await
            .unwrap();

        assert_eq!(second.version.to_string(), "1.0.1");
        assert_eq!(second.parent_id, Some(first.id.clone()));

        let updated = store.get_prompt(&first.prompt_id).await.unwrap();
        assert_eq!(updated.head_commit_id, second.id);
    }

    #[tokio::test]
    async fn blob_store_roundtrip() {
        let store = test_store().await;
        let data = b"test blob content";
        let hash = store.put_blob(data).await.unwrap();
        assert!(hash.to_string().starts_with("sha256:"));
        let back = store.get_blob(&hash).await.unwrap();
        assert_eq!(back, data);
    }

    #[tokio::test]
    async fn event_log_populated() {
        let store = test_store().await;
        let owner = test_account();
        let space = store
            .create_space(CreateSpaceInput {
                owner_id: owner.clone(),
                slug: "evtest".into(),
                description: None,
                is_public: true,
            })
            .await
            .unwrap();

        let content = b"Simple prompt body";
        store
            .create_prompt(CommitInput {
                space_id: space.id.clone(),
                author_id: owner.clone(),
                prompt_id: None,
                slug: Some("simple".into()),
                raw_bytes: content.to_vec(),
                message: None,
                bump: BumpKind::Patch,
            })
            .await
            .unwrap();

        let events = store
            .list_events(&space.id, EventCursor::from_start(), 10)
            .await
            .unwrap();

        // SpaceCreated + PromptCreated
        assert!(events.len() >= 2);
        assert!(events.iter().any(|e| matches!(e.payload, EventPayload::SpaceCreated { .. })));
        assert!(events.iter().any(|e| matches!(e.payload, EventPayload::PromptCreated { .. })));
    }

    #[tokio::test]
    async fn list_prompts_pagination() {
        let store = test_store().await;
        let owner = test_account();
        let space = store
            .create_space(CreateSpaceInput {
                owner_id: owner.clone(),
                slug: "paged".into(),
                description: None,
                is_public: true,
            })
            .await
            .unwrap();

        for i in 0..5u8 {
            let slug = format!("prompt-{}", i);
            let raw = format!("body {}", i).into_bytes();
            store
                .create_prompt(CommitInput {
                    space_id: space.id.clone(),
                    author_id: owner.clone(),
                    prompt_id: None,
                    slug: Some(slug),
                    raw_bytes: raw,
                    message: None,
                    bump: BumpKind::Patch,
                })
                .await
                .unwrap();
        }

        let page1 = store
            .list_prompts(
                &space.id,
                PromptFilter::default(),
                PageRequest { cursor: None, limit: 3 },
            )
            .await
            .unwrap();
        assert_eq!(page1.items.len(), 3);
        assert!(page1.next_cursor.is_some());

        let page2 = store
            .list_prompts(
                &space.id,
                PromptFilter::default(),
                PageRequest { cursor: page1.next_cursor, limit: 3 },
            )
            .await
            .unwrap();
        assert_eq!(page2.items.len(), 2);
        assert!(page2.next_cursor.is_none());
    }
}

