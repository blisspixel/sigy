# Changelog

## Unreleased

- Added a Rust workspace with a dependency-free domain core, transactional service storage, and an initial maintenance CLI.
- Added exact USD accounting, global and named lifetime budgets, atomic reservations, submission uncertainty, idempotent reconciliation, and billing-breach freezes. Provider dispatch remains unavailable.
- Added exclusive library ownership, schema identity checks, SQLite migrations, and append-only ledger events.
- Added a bounded local controller with foreground/detached operation, client reconnect, explicit shutdown, protected local IPC, and restart reconciliation. Native Windows behavior is tested; OS startup installation remains pending.
- Added capture intent, atomic admission, revision/generation checks, append-only job history, interrupted-attempt recovery, integrity audits and bounded reads.
- Added schema v3 immutable HTTP source revisions, exact replay/conflict handling, explicit network grants and paginated CLI registration/list/inspection through the shared controller.
- Added finite HTTP acquisition with checked destinations, disabled implicit proxies/retries, explicit bounded redirect policy, strict response handling, byte/time limits and shared admission. Successful recordings retain origin/peer/status provenance in metadata envelope v2.
- Added service-owned finite recordings with decoder validation, original-byte hashes, 14-day/50 GB rolling retention, Keep/Archive protection, processing acknowledgments and staged deletion.
- Added bounded Radio Browser refresh, cached multilingual search and immutable source registration with directory provenance. Refresh does not contact station streams.
- Added persistent radio favorites and filtered offline search through shared service/maintenance operations. Catalog and IPC are v7; saved choices survive refresh and restart.
- Added truncated-catalog rejection, conservative build/test concurrency, bounded CLI test processes, and regression coverage for detached output-handle inheritance.
- Added local tests for accounting, concurrency, restart recovery, injected storage failures, library locking, and real CLI operations.
- Pinned Rust 1.98.1 and verified SQLite 3.53.4 source through a documented temporary native dependency override.
- Extended the roadmap with optional user-controlled proxy routing and external VPN compatibility for personal machines and servers. Routing profiles, remote access, podcasts, integrated playback, TUI, model processing and monitoring remain unimplemented.
