# syntax=docker/dockerfile:1
# ─────────────────────────────────────────────────────────────
# Intentry - intr CLI binary
# Produces a minimal image suitable for pinning in CI pipelines
# or running as a sidecar.
# ─────────────────────────────────────────────────────────────

# ── Stage 1: Rust build ───────────────────────────────────────
FROM rust:1.94-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --bin intr && \
    cp /app/target/release/intr /usr/local/bin/intr

# ── Stage 2: Minimal runtime image ───────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -r -s /bin/false intentry

COPY --from=builder /usr/local/bin/intr /usr/local/bin/intr

USER intentry

# Default: print help. Override with a command like:
#   docker run ghcr.io/intentry/intentry:latest intr commit
ENTRYPOINT ["/usr/local/bin/intr"]
CMD ["--help"]
