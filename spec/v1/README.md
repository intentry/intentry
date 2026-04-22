# .prompt Specification — v1

> The authoritative source for the Intentry `.prompt` file format.

**Status:** Draft — V1 launch target
**Maintained by:** Intentry / B3M Studio (stewardship transfer planned for V1.5)

---

## Overview

The `.prompt` file format is a plain-text, human-readable format for capturing the full cognitive contract between a human developer and an AI model. It is:

- **100% Dotprompt-compatible** — any valid Dotprompt file is a valid `.prompt` file
- **Progressively structured** — Tier 1 works with zero metadata; Tier 3 unlocks full platform features
- **Versionable** — designed for git-like version control from the ground up

## Quick reference

See [grammar.md](grammar.md) for the full grammar and [examples/](examples/) for annotated examples.

## Tiers

| Tier | What you need | What you unlock |
|---|---|---|
| 1 | Just a template body (Handlebars) | Basic version tracking, variable extraction |
| 2 | YAML frontmatter with `id` + `version` | Model hints, typed inputs, reputation scoring |
| 3 | Full frontmatter with `evals` | Automated testing, drift detection, commons listing |

## File format at a glance

```
---
id: my-prompt
version: 1.0.0
description: What this prompt does
model:
  preferred: [claude-sonnet-4-6, gpt-4o]
  temperature: 0.3
input:
  schema:
    name: string
    context?: string
intentry:
  tags: [summarization, text]
  license: MIT
evals:
  - description: Returns a greeting
    input: { name: "Alice" }
    expect:
      contains: "Alice"
---
Hello, {{name}}!
{{#if context}}Context: {{context}}{{/if}}
```

## Implementation status

- [x] Tier 1 parsing (intr-parser v0.1.0)
- [x] Tier 2 parsing
- [ ] Tier 3 evals runner (V1-019)
- [ ] Conformance test suite (V1-017)
