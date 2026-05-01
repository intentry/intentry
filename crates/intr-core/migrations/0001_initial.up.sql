-- V1-001 Phase 3 - RemoteStore initial schema
-- Run against Neon (Postgres 18). Safe to re-run (IF NOT EXISTS guards).

CREATE TABLE IF NOT EXISTS spaces (
    id          TEXT        PRIMARY KEY,
    owner_id    TEXT        NOT NULL,
    slug        TEXT        NOT NULL,
    description TEXT,
    is_public   BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL,
    UNIQUE (owner_id, slug)
);

CREATE TABLE IF NOT EXISTS prompts (
    id               TEXT        PRIMARY KEY,
    space_id         TEXT        NOT NULL REFERENCES spaces(id),
    slug             TEXT        NOT NULL,
    head_commit_id   TEXT        NOT NULL,
    current_version  TEXT        NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL,
    updated_at       TIMESTAMPTZ NOT NULL,
    UNIQUE (space_id, slug)
);

CREATE TABLE IF NOT EXISTS commits (
    id            TEXT        PRIMARY KEY,
    prompt_id     TEXT        NOT NULL REFERENCES prompts(id),
    space_id      TEXT        NOT NULL REFERENCES spaces(id),
    author_id     TEXT        NOT NULL,
    content_hash  TEXT        NOT NULL,
    version       TEXT        NOT NULL,
    message       TEXT,
    parent_id     TEXT,
    created_at    TIMESTAMPTZ NOT NULL,
    UNIQUE (prompt_id, version)
);

-- Append-only event log. `payload` is JSONB for query flexibility.
CREATE TABLE IF NOT EXISTS events (
    seq          BIGSERIAL   PRIMARY KEY,
    occurred_at  TIMESTAMPTZ NOT NULL,
    actor_id     TEXT        NOT NULL,
    space_id     TEXT        NOT NULL,
    payload      JSONB       NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_prompts_space       ON prompts(space_id);
CREATE INDEX IF NOT EXISTS idx_commits_prompt      ON commits(prompt_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_events_space_seq    ON events(space_id, seq);
