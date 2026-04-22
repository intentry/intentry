# FOUNDATIONS.md — The 10 Laws of Intentry

> This is the pinned copy of S-000. The authoritative spec is in `intentry/specs`.
> If a design decision conflicts with any law here, the implementation is wrong — not the law.

---

## The 10 Laws

### 1. Open standard before platform

The `.prompt` specification must be publishable, readable, and implementable by anyone — including competitors — without any Intentry account, login, or SDK. If a design decision makes the standard harder to adopt outside Intentry, it is wrong.

### 2. Local-first, cloud-optional

The CLI must work fully offline against a local `.intr/` directory. The cloud is a sync target, not a prerequisite. A developer with no internet can version, diff, run (against local Ollama), and eval prompts.

### 3. Event-sourced state

Every mutation to core domain state (prompts, versions, forks, runs) is an append-only event. State is a projection over events. This unlocks replay, audit, time-travel, migration previews, and reputation recalculation.

### 4. API-first, no private endpoints

Every capability in the web UI must be exposed via the public API. If it's in the product, it's in the API. This is how third parties build on us.

### 5. Dotprompt-compatible, Dotprompt-super

Any valid Dotprompt file must be a valid `.prompt` file in Intentry. We extend, we never break backward compatibility.

### 6. Boring infrastructure

Postgres, object storage, Cloudflare, standard queues. No exotic distributed databases. No premature Kubernetes.

### 7. Typed contracts everywhere

OpenAPI for HTTP. Zod/serde for runtime validation. No untyped JSON blobs crossing service boundaries.

### 8. OSS core, hosted runtime

The version store, diff engine, CLI, SDK, and `.prompt` parser are **MIT-licensed OSS from day one**. The runtime API, hosted web app, reputation engine, and enterprise features are closed-source hosted products. This boundary is maintained by the two-repo split — never cross it.

### 9. Cheap by default

Every design decision should make unit cost go down as we scale. Edge caching over origin. Batch over per-event. Hetzner-class compute over hyperscaler when usage is predictable.

### 10. Founder-voiced, developer-first

No marketing copy in product surfaces. The CLI, docs, error messages, web UI — everything speaks like the founder talking to a developer peer. Specific, opinionated, sharp, sometimes funny. Never bland.

---

## Decision framework

When making any technical decision, apply in this order:

1. **Does it violate any of the 10 Laws?** → If yes, reject.
2. **Does it accelerate standard adoption or slow it?** → Accelerate.
3. **Does it reduce unit cost at scale?** → Prefer this path.
4. **Is it reversible within one sprint?** → Prefer reversible choices early.
5. **Would the founder be proud to explain this in a blog post?** → Final sanity check.

---

*Source: S-000 Foundation Principles · intentry/specs · 2026*
