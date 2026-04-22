# Contributing to Intentry

Thank you for your interest. This project is spec-driven — every contribution starts with a spec, not with code.

## Before you start

1. Read [FOUNDATIONS.md](FOUNDATIONS.md) — the 10 Laws govern every decision here.
2. Find the relevant spec in [intentry/specs](https://github.com/intentry/specs) for what you want to build.
3. If no spec exists for your feature, open an issue describing the problem. We'll write the spec together before any code is written.

## Contribution workflow

1. **Find the spec** — identify the spec ID (e.g. `V1-002`) that covers your change.
2. **Fork the repo** and create a branch: `feat/<spec-id>-<short-slug>` or `fix/<spec-id>-<short-slug>`.
3. **Implement** — follow the spec exactly. If you think the spec is wrong, open an ADR (see below).
4. **Write tests** — unit tests for all new logic. No PR merges below the coverage floor.
5. **Open a PR** — title format: `V1-002: <description>`. Reference the spec ID in the PR body.
6. **Wait for CI** — all checks must be green before review.

## Commit message format

```
V1-002: implement semantic diff for template changes
fix/V1-001: handle empty frontmatter in LocalStore
```

Prefix with the spec ID. Keep it short and imperative.

## When the spec is wrong

Don't silently deviate. Instead:

1. Stop. Do not implement your own version of the spec.
2. Open a file: `shared/adrs/<date>-<short-description>.md` in the specs repo.
3. Format: Problem | Current spec says | Why it's wrong | What I propose | Impact.
4. Open a GitHub issue referencing the ADR and tag it `spec-correction`.
5. Wait for resolution before continuing.

## Code standards

- No `unwrap()` in library code. Use `?` or explicit error handling.
- No `panic!` in library crates. Only in CLI binary for genuine invariants.
- Every public function has a `///` doc comment.
- No commented-out code blocks in PRs.
- No `TODO` comments merged to `main`. Either do it now or open a GitHub issue.
- Run `cargo clippy --workspace -- -D warnings` before pushing. CI enforces this.
- Run `cargo fmt --all` before pushing.

## Rust specifics

- Edition 2024. MSRV 1.94.
- Async: `tokio` only. No `async-std`.
- Errors: `thiserror` for library errors, `anyhow` for CLI/binary errors.
- HTTP client (if needed): `reqwest` with `rustls-tls` feature.
- Serialization: `serde` + `serde_json` for JSON, `serde_yaml` for YAML.

## What we won't accept

- Features without a spec.
- Breaking changes to the `.prompt` format or public API without a versioning plan.
- Anything that violates the 10 Laws.
- Dependencies that add a commercial license to the OSS core.

## Code of Conduct

See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). Short version: be direct, be respectful, no nonsense.
