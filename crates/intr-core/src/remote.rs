/// Cloud-backed store using PostgreSQL (Neon) + Cloudflare R2.
///
/// `RemoteStore` implements [`crate::store::VersionStore`] using:
/// - **PostgreSQL** (via `sqlx`) for structured data (spaces, prompts, commits,
///   events). Target: [Neon](https://neon.tech) Postgres 18.
/// - **Cloudflare R2** (S3-compatible) for content-addressed blob storage.
///
/// R2 object key layout:
///
/// ```text
/// sha256/<first2>/<rest>   e.g. sha256/ab/cdef1234...
/// ```
///
/// # Configuration
///
/// Construct via [`RemoteStoreConfig`] - all fields are plain strings so
/// the caller controls how secrets are sourced (Doppler, env, etc.).
/// Never hard-code credentials.
///
/// # Feature flag
///
/// Compiled only when the `postgres` feature is enabled.
use chrono::Utc;
use sqlx::{
    postgres::{PgPoolOptions, PgRow},
    PgPool, Row,
};

use aws_config::Region;
use aws_credential_types::Credentials;
use aws_sdk_s3::{
    Client as S3Client,
    config::Builder as S3Builder,
    primitives::ByteStream,
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
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for [`RemoteStore`].
///
/// All sensitive fields (access keys, database URL) must come from Doppler
/// or environment variables - never from source code.
#[derive(Debug, Clone)]
pub struct RemoteStoreConfig {
    /// Postgres connection string, e.g. `postgresql://user:pass@host/db?sslmode=require`
    pub database_url: String,
    /// Cloudflare R2 S3-compatible endpoint, e.g.
    /// `https://<account-id>.r2.cloudflarestorage.com`
    pub r2_endpoint: String,
    /// R2 bucket name.
    pub r2_bucket: String,
    /// R2 access key ID.
    pub r2_access_key_id: String,
    /// R2 secret access key.
    pub r2_secret_access_key: String,
    /// Maximum Postgres connections in the pool. Defaults to 10.
    pub max_connections: u32,
}

impl RemoteStoreConfig {
    /// Build config from environment variables:
    ///
    /// - `DATABASE_URL`
    /// - `R2_ENDPOINT`
    /// - `R2_BUCKET`
    /// - `R2_ACCESS_KEY_ID`
    /// - `R2_SECRET_ACCESS_KEY`
    pub fn from_env() -> Result<Self, StorageError> {
        fn env(key: &str) -> Result<String, StorageError> {
            std::env::var(key).map_err(|_| {
                StorageError::Configuration(format!("missing required env var: {key}"))
            })
        }
        Ok(Self {
            database_url: env("DATABASE_URL")?,
            r2_endpoint: env("R2_ENDPOINT")?,
            r2_bucket: env("R2_BUCKET")?,
            r2_access_key_id: env("R2_ACCESS_KEY_ID")?,
            r2_secret_access_key: env("R2_SECRET_ACCESS_KEY")?,
            max_connections: 10,
        })
    }
}

// ---------------------------------------------------------------------------
// RemoteStore
// ---------------------------------------------------------------------------

/// Cloud store: PostgreSQL projection + Cloudflare R2 blob storage.
#[derive(Clone)]
pub struct RemoteStore {
    pool: PgPool,
    r2: S3Client,
    bucket: String,
}

impl RemoteStore {
    /// Create a new `RemoteStore`.
    ///
    /// Runs schema migrations (idempotent `CREATE TABLE IF NOT EXISTS`) against
    /// Postgres on every call - safe for cold starts and deployments.
    pub async fn new(config: RemoteStoreConfig) -> Result<Self, StorageError> {
        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .connect(&config.database_url)
            .await
            .map_err(|e| StorageError::Postgres(e.to_string()))?;

        Self::run_migrations(&pool).await?;

        let creds = Credentials::new(
            &config.r2_access_key_id,
            &config.r2_secret_access_key,
            None, // session token
            None, // expiry
            "r2",
        );

        let r2_config = S3Builder::new()
            .region(Region::new("auto"))
            .endpoint_url(&config.r2_endpoint)
            .credentials_provider(creds)
            .force_path_style(true)
            .build();

        let r2 = S3Client::from_conf(r2_config);

        Ok(Self {
            pool,
            r2,
            bucket: config.r2_bucket,
        })
    }

    /// Run idempotent schema migrations.
    ///
    /// Uses inline SQL (same as `SCHEMA_STMTS` in `local.rs`) so the crate
    /// compiles without a live `DATABASE_URL`. Production deploys should
    /// also run `migrations/0001_initial.up.sql` via `sqlx migrate run`.
    async fn run_migrations(pool: &PgPool) -> Result<(), StorageError> {
        let migration_sql = include_str!("../migrations/0001_initial.up.sql");
        // Execute each statement individually - sqlx execute doesn't support
        // multi-statement strings in all configurations.
        for stmt in migration_sql
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            sqlx::query(stmt)
                .execute(pool)
                .await
                .map_err(|e| StorageError::Postgres(e.to_string()))?;
        }
        Ok(())
    }

    // -- R2 blob operations -------------------------------------------------

    fn r2_key(&self, hash: &ContentHash) -> String {
        let hex = hash.hex();
        let (prefix, rest) = hex.split_at(2);
        format!("sha256/{}/{}", prefix, rest)
    }

    async fn write_blob_bytes(
        &self,
        hash: &ContentHash,
        bytes: &[u8],
    ) -> Result<(), StorageError> {
        let key = self.r2_key(hash);
        // HEAD to check existence before writing (content-addressed - idempotent).
        let exists = self
            .r2
            .head_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .is_ok();
        if exists {
            return Ok(());
        }

        let body = ByteStream::from(bytes.to_vec());
        self.r2
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(body)
            .send()
            .await
            .map_err(|e| StorageError::R2(e.to_string()))?;
        Ok(())
    }

    async fn read_blob_bytes(&self, hash: &ContentHash) -> Result<Vec<u8>, StorageError> {
        let key = self.r2_key(hash);
        let resp = self
            .r2
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .map_err(|e| {
                // Distinguish NotFound from other errors.
                let msg = e.to_string();
                if msg.contains("NoSuchKey") || msg.contains("404") {
                    StorageError::R2(format!("blob not found: {}", hash))
                } else {
                    StorageError::R2(msg)
                }
            })?;

        let bytes = resp
            .body
            .collect()
            .await
            .map_err(|e| StorageError::R2(e.to_string()))?
            .into_bytes()
            .to_vec();
        Ok(bytes)
    }

    // -- Event append -------------------------------------------------------

    async fn append_event(
        &self,
        actor_id: &AccountId,
        space_id: &SpaceId,
        payload: &EventPayload,
    ) -> Result<u64, StorageError> {
        let payload_json = serde_json::to_value(payload)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        let now = Utc::now();

        let row: (i64,) = sqlx::query_as(
            "INSERT INTO events (occurred_at, actor_id, space_id, payload)
             VALUES ($1, $2, $3, $4)
             RETURNING seq",
        )
        .bind(now)
        .bind(actor_id.to_string())
        .bind(space_id.to_string())
        .bind(payload_json)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Postgres(e.to_string()))?;

        Ok(row.0 as u64)
    }

    // -- Version resolution (identical logic to LocalStore) -----------------

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
// Row types
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct SpaceRow {
    id: String,
    owner_id: String,
    slug: String,
    description: Option<String>,
    is_public: bool,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct PromptRow {
    id: String,
    space_id: String,
    slug: String,
    head_commit_id: String,
    current_version: String,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
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
    created_at: chrono::DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Row → domain conversions (much simpler than LocalStore - no string parsing
// for timestamps since Postgres TIMESTAMPTZ maps directly to DateTime<Utc>).
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
        is_public: r.is_public,
        created_at: r.created_at,
        updated_at: r.updated_at,
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
        created_at: r.created_at,
        updated_at: r.updated_at,
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
        created_at: r.created_at,
    })
}

// ---------------------------------------------------------------------------
// VersionStore implementation
// ---------------------------------------------------------------------------

impl VersionStore for RemoteStore {
    // -- Spaces -------------------------------------------------------------

    async fn create_space(
        &self,
        input: CreateSpaceInput,
    ) -> Result<Space, VersionStoreError> {
        let id = SpaceId::new();
        let now = Utc::now();

        sqlx::query(
            "INSERT INTO spaces (id, owner_id, slug, description, is_public, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(id.to_string())
        .bind(input.owner_id.to_string())
        .bind(&input.slug)
        .bind(&input.description)
        .bind(input.is_public)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db) if db.is_unique_violation() => {
                VersionStoreError::Conflict(format!("space '{}' already exists", input.slug))
            }
            e => VersionStoreError::Storage(StorageError::Postgres(e.to_string())),
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
        let row = sqlx::query_as::<_, SpaceRow>("SELECT * FROM spaces WHERE id = $1")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?
            .ok_or_else(|| VersionStoreError::NotFound(format!("space {id}")))?;
        space_from_row(row).map_err(VersionStoreError::Storage)
    }

    async fn get_space_by_slug(
        &self,
        owner_id: &AccountId,
        slug: &str,
    ) -> Result<Space, VersionStoreError> {
        let row = sqlx::query_as::<_, SpaceRow>(
            "SELECT * FROM spaces WHERE owner_id = $1 AND slug = $2",
        )
        .bind(owner_id.to_string())
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?
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
        let ver_str = version.to_string();
        let hash_str = content_hash.to_string();

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?;

        // Insert prompt first (FK: commits.prompt_id → prompts.id).
        sqlx::query(
            "INSERT INTO prompts (id, space_id, slug, head_commit_id, current_version, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(prompt_id.to_string())
        .bind(input.space_id.to_string())
        .bind(slug)
        .bind(commit_id.to_string())
        .bind(&ver_str)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db) if db.is_unique_violation() => {
                VersionStoreError::Conflict(format!(
                    "prompt '{}' already exists in this space",
                    slug
                ))
            }
            e => VersionStoreError::Storage(StorageError::Postgres(e.to_string())),
        })?;

        sqlx::query(
            "INSERT INTO commits (id, prompt_id, space_id, author_id, content_hash, version, message, parent_id, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, NULL, $8)",
        )
        .bind(commit_id.to_string())
        .bind(prompt_id.to_string())
        .bind(input.space_id.to_string())
        .bind(input.author_id.to_string())
        .bind(&hash_str)
        .bind(&ver_str)
        .bind(&input.message)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?;

        tx.commit()
            .await
            .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?;

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
            VersionStoreError::Validation(
                "prompt_id is required when committing to an existing prompt".into(),
            )
        })?;

        let prompt = self.get_prompt(prompt_id).await?;

        let parsed = intr_parser::parse(&input.raw_bytes)
            .map_err(|e| VersionStoreError::Validation(e.to_string()))?;
        let new_version =
            self.resolve_new_version(&parsed, Some(&prompt.current_version), input.bump)?;

        if new_version <= prompt.current_version {
            return Err(VersionStoreError::Conflict(format!(
                "new version {} must be greater than current version {}",
                new_version, prompt.current_version
            )));
        }

        let content_hash = ContentHash::of(&input.raw_bytes);

        // Idempotency: same content as head → return existing head commit.
        let head = self.get_commit(&prompt.head_commit_id).await?;
        if content_hash == head.content_hash {
            return Ok(head);
        }

        self.write_blob_bytes(&content_hash, &input.raw_bytes)
            .await
            .map_err(VersionStoreError::Storage)?;

        let commit_id = CommitId::new();
        let now = Utc::now();
        let ver_str = new_version.to_string();
        let hash_str = content_hash.to_string();
        let parent_commit_id = prompt.head_commit_id.clone();

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?;

        sqlx::query(
            "INSERT INTO commits (id, prompt_id, space_id, author_id, content_hash, version, message, parent_id, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(commit_id.to_string())
        .bind(prompt_id.to_string())
        .bind(input.space_id.to_string())
        .bind(input.author_id.to_string())
        .bind(&hash_str)
        .bind(&ver_str)
        .bind(&input.message)
        .bind(parent_commit_id.to_string())
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db) if db.is_unique_violation() => {
                VersionStoreError::Conflict(format!(
                    "version {} already exists on this prompt",
                    ver_str
                ))
            }
            e => VersionStoreError::Storage(StorageError::Postgres(e.to_string())),
        })?;

        sqlx::query(
            "UPDATE prompts SET head_commit_id = $1, current_version = $2, updated_at = $3 WHERE id = $4",
        )
        .bind(commit_id.to_string())
        .bind(&ver_str)
        .bind(now)
        .bind(prompt_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?;

        tx.commit()
            .await
            .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?;

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

        let forked_bytes = inject_fork_attribution(&source_blob, &source_commit)
            .map_err(|e| VersionStoreError::Validation(e.to_string()))?;

        let commit_input = CommitInput {
            space_id: input.target_space_id.clone(),
            author_id: input.author_id.clone(),
            prompt_id: None,
            slug: Some(input.new_slug),
            raw_bytes: forked_bytes,
            message: Some("forked".to_string()),
            bump: BumpKind::Explicit,
        };

        let result = self.create_prompt(commit_input.clone()).await;
        match result {
            Ok(commit) => {
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
            sqlx::query_as::<_, PromptRow>("SELECT * FROM prompts WHERE id = $1")
                .bind(id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?
                .ok_or_else(|| VersionStoreError::NotFound(format!("prompt {id}")))?;
        prompt_from_row(row).map_err(VersionStoreError::Storage)
    }

    async fn get_prompt_by_slug(
        &self,
        space_id: &SpaceId,
        slug: &str,
    ) -> Result<Prompt, VersionStoreError> {
        let row = sqlx::query_as::<_, PromptRow>(
            "SELECT * FROM prompts WHERE space_id = $1 AND slug = $2",
        )
        .bind(space_id.to_string())
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?
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
        let after_id = page.cursor.as_deref().unwrap_or("").to_string();

        let rows: Vec<PromptRow> = if let Some(ref q) = filter.query {
            let pattern = format!("%{}%", q);
            sqlx::query_as::<_, PromptRow>(
                "SELECT * FROM prompts WHERE space_id = $1 AND slug ILIKE $2 AND id > $3
                 ORDER BY id LIMIT $4",
            )
            .bind(space_id.to_string())
            .bind(&pattern)
            .bind(&after_id)
            .bind(limit + 1)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as::<_, PromptRow>(
                "SELECT * FROM prompts WHERE space_id = $1 AND id > $2 ORDER BY id LIMIT $3",
            )
            .bind(space_id.to_string())
            .bind(&after_id)
            .bind(limit + 1)
            .fetch_all(&self.pool)
            .await
        }
        .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?;

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
            sqlx::query_as::<_, CommitRow>("SELECT * FROM commits WHERE id = $1")
                .bind(id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?
                .ok_or_else(|| VersionStoreError::NotFound(format!("commit {id}")))?;
        commit_from_row(row).map_err(VersionStoreError::Storage)
    }

    async fn list_commits(
        &self,
        prompt_id: &PromptId,
        page: PageRequest,
    ) -> Result<Page<Commit>, VersionStoreError> {
        let limit = page.limit.min(100) as i64;
        let after_id = page.cursor.as_deref().unwrap_or("").to_string();

        let rows: Vec<CommitRow> = sqlx::query_as::<_, CommitRow>(
            "SELECT * FROM commits WHERE prompt_id = $1 AND id > $2
             ORDER BY created_at DESC LIMIT $3",
        )
        .bind(prompt_id.to_string())
        .bind(&after_id)
        .bind(limit + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?;

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
             FROM events WHERE space_id = $1 AND seq > $2
             ORDER BY seq ASC LIMIT $3",
        )
        .bind(space_id.to_string())
        .bind(from.seq as i64)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?;

        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            let seq: i64 = row.try_get("seq")
                .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?;
            let occurred_at: chrono::DateTime<Utc> = row.try_get("occurred_at")
                .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?;
            let actor_id_str: &str = row.try_get("actor_id")
                .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?;
            let space_id_str: &str = row.try_get("space_id")
                .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?;
            let payload_val: serde_json::Value = row.try_get("payload")
                .map_err(|e| VersionStoreError::Storage(StorageError::Postgres(e.to_string())))?;

            let payload: EventPayload =
                serde_json::from_value(payload_val).map_err(|e| {
                    VersionStoreError::Storage(StorageError::Serialization(e.to_string()))
                })?;

            events.push(Event {
                seq: seq as u64,
                occurred_at,
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
// Fork attribution (mirrors local.rs implementation)
// ---------------------------------------------------------------------------

fn inject_fork_attribution(
    source_bytes: &[u8],
    source_commit: &Commit,
) -> Result<Vec<u8>, String> {
    let src = std::str::from_utf8(source_bytes).map_err(|e| e.to_string())?;
    let parent_ref = format!("{}@{}", source_commit.prompt_id, source_commit.version);
    let forked_at = Utc::now().to_rfc3339();
    let (yaml_opt, body) = split_frontmatter_raw(src);

    let new_yaml = match yaml_opt {
        Some(yaml_str) => {
            let mut value: serde_yaml::Value =
                serde_yaml::from_str(yaml_str).map_err(|e| e.to_string())?;
            let map = value
                .as_mapping_mut()
                .ok_or("frontmatter is not a YAML mapping")?;

            let intentry_key = serde_yaml::Value::String("intentry".into());
            let intentry = map
                .entry(intentry_key)
                .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));

            if let Some(im) = intentry.as_mapping_mut() {
                im.insert("parent".into(), serde_yaml::Value::String(parent_ref));
                im.insert("forked_at".into(), serde_yaml::Value::String(forked_at));
            }
            serde_yaml::to_string(&value).map_err(|e| e.to_string())?
        }
        None => format!(
            "version: \"1.0.0\"\nintentry:\n  parent: \"{}\"\n  forked_at: \"{}\"\n",
            parent_ref, forked_at
        ),
    };

    Ok(format!("---\n{}---\n{}", new_yaml, body).into_bytes())
}

fn split_frontmatter_raw(src: &str) -> (Option<&str>, &str) {
    let src = src.trim_start();
    if !src.starts_with("---") {
        return (None, src);
    }
    let after_open = src[3..].trim_start_matches([' ', '\t', '\r', '\n']);
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
