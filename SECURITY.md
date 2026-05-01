# Security Policy

## Supported versions

| Version | Supported |
|---------|-----------|
| Latest  | ✅        |
| Older   | ❌        |

We support security patches on the latest released version only.

## Reporting a vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Email: security@intentry.dev

Include:
- Description of the vulnerability
- Steps to reproduce
- Impact assessment
- Any suggested mitigations

We will acknowledge within 48 hours and aim to release a fix within 14 days for critical issues.

## Scope

In scope:
- `intr-core`, `intr-parser`, `intr-cli`, SDK libraries
- The `.prompt` spec - design flaws that enable parser attacks

Out of scope:
- The hosted platform (`intentry.dev`) - report via HackerOne (link TBD)
- Third-party dependencies - report directly to upstream

## Disclosure policy

We follow responsible disclosure. We will credit researchers in release notes unless they request anonymity.
