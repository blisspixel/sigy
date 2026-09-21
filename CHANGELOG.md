# Changelog

## Unreleased

- Added a Rust workspace with a dependency-free domain core, transactional service storage, and an initial maintenance CLI.
- Added exact USD accounting, global and named lifetime budgets, atomic reservations, submission uncertainty, idempotent reconciliation, and billing-breach freezes. Provider dispatch remains unavailable.
- Added exclusive library ownership, schema identity checks, SQLite migrations, and append-only ledger events.
- Added a bounded local controller with foreground/detached operation, client reconnect, explicit shutdown, protected local IPC, and restart reconciliation. Native Windows behavior is tested; OS startup installation remains pending.
- Added schema v2 capture intent, atomic admission, revision/generation checks, append-only job history, interrupted-attempt recovery, integrity audits and bounded reads. Service-owned acquisition and capture completion remain unavailable.
- Added schema v3 immutable HTTP source revisions, exact replay/conflict handling, explicit network grants and paginated CLI registration/list/inspection through the shared controller.
- Added an internal finite HTTP adapter with pre-connection destination checks, disabled implicit proxies/retries/redirects, strict response handling, byte/time limits, shared admission and local HTTP/TLS failure fixtures. A transfer receipt does not claim verified or durable media; recording integration remains pending.
- Added truncated-catalog rejection, conservative build/test concurrency, bounded CLI test processes, and regression coverage for detached output-handle inheritance.
- Added local tests for accounting, concurrency, restart recovery, injected storage failures, library locking, and real CLI operations.
- Pinned Rust 1.98.1 and verified SQLite 3.53.4 source through a documented temporary native dependency override.
- Preserved the complete radio, multilingual processing, terminal explorer, and monitoring roadmap. These workflows are not yet available.
