# Intentry

> Version control for intent. An open protocol and public commons for human–AI communication.

[![CI](https://github.com/intentry/intentry/actions/workflows/ci.yml/badge.svg)](https://github.com/intentry/intentry/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/intr-core.svg)](https://crates.io/crates/intr-core)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

---

## What this is

Intentry is the open protocol for prompt version control. It defines:

- **`.prompt` file format** — a Dotprompt-compatible, progressively structured file format for capturing the full cognitive contract between human and model
- **Version store** — event-sourced, content-addressed storage for prompt history
- **Diff engine** — semantic diffing between prompt versions
- **`intr` CLI** — version, commit, fork, and run `.prompt` files locally or against the hosted commons
- **SDKs** — TypeScript, Python, and Go clients for the public API

The hosted platform (`intentry.dev`) is built on top of these open-source primitives. Everything here is MIT-licensed and self-hostable.

## Quick start

```bash
# Install the CLI
cargo install intr-cli

# Initialize a space in your project
intr init

# Write a prompt
cat > summarize.prompt << 'EOF'
---
id: summarize
version: 1.0.0
description: One-sentence summary of arbitrary text
model:
  preferred: [claude-sonnet-4-6, gpt-4o]
  temperature: 0.2
input:
  schema:
    text: string
---
Summarize the following in one sentence.

Text: {{text}}
EOF

# Commit it
intr commit summarize.prompt

# Run it against a model
intr run summarize.prompt --input '{"text": "Your input here"}'
```

## Repository structure

```
intentry/
├── spec/           # The .prompt specification (hosted at intentry.dev/spec)
├── crates/
│   ├── intr-cli            # CLI binary: `intr`
│   ├── intr-core           # Core engine: version store, event log, projections
│   ├── intr-parser         # .prompt file parser (Tier 1–3)
│   ├── intr-runtime-local  # Local model execution (Ollama, llama.cpp)
│   └── intr-providers      # Model provider abstraction
├── sdk/
│   ├── typescript/         # @intentry/sdk (npm)
│   ├── python/             # intentry (PyPI)
│   └── go/                 # github.com/intentry/go-sdk
└── extensions/
    └── vscode/             # VSCode/Cursor extension
```

## Building

```bash
# Build everything
cargo build --workspace

# Run tests
cargo test --workspace

# Check with Clippy
cargo clippy --workspace -- -D warnings

# Build the CLI binary
cargo build -p intr-cli --release
```

## The 10 Laws

Every decision in this codebase is governed by the 10 Foundation Principles in [FOUNDATIONS.md](FOUNDATIONS.md). Read them before contributing.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The short version: spec first, then code; PRs reference a spec ID.

## License

MIT — see [LICENSE](LICENSE).
