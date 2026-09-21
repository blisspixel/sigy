# Implementation progress

Updated: 2026-09-20. Status: active evaluation and implementation, authorized after the planning checkpoint.

## Current objective

Build the planned product in sound, verifiable increments. Keep Sigy as the working name. The complete first release still requires radio discovery, recordings/DVR, live multilingual translation, and bounded topic monitoring through the CLI and optional TUI. An internal foundation milestone is not a release.

## Work sequence

| Work | State | Acceptance evidence |
| --- | --- | --- |
| Foundation technology decision | Selected for initial development | Rust 1.98.1 and SQLite 3.53.4; reproducible Rust/Go probes, current stable dependency review, scoped decision and explicit platform limitations |
| Domain and durable catalog | Initial ledger implemented and tested | Exact amounts, capture state machine, multi-scope atomic reservations, replay protection, restart uncertainty, overcharge freeze, rollback faults, exclusive library ownership, foreign/future database rejection |
| Persistent service and CLI | Local Windows controller implemented and tested | Foreground/detached operation, reconnecting maintenance commands, explicit stop, malformed/oversized/versioned IPC, peer process identity, output-pipe closure, restart and single-owner tests; native Unix and startup installation remain pending |
| Durable capture journal | Internal lifecycle API implemented and tested | Schema v2 migration, finite intent, idempotency, atomic admission and journal writes, stale-worker rejection, interrupted recovery, bounded history and corruption checks; no source worker or media completion yet |
| Internet radio and recording | Planned | Bounded catalog/network inputs, source policy, finite capture, durable segments, failure/restart fixtures |
| Terminal explorer and DVR | Planned | CLI parity, rendered terminal evidence, safe multilingual text, map/globe, retention and timeline tests |
| Local processing and monitoring | Planned | Typed provider boundaries, real local smoke results, per-language limits, grounded outputs and policy tests |

Follow the detailed experiment and assurance registers in `docs/planning/`. Initial implementation defaults can bound open questions without claiming universal support: captions before synthesized speech, explicit finite capture limits, foreground service operation before OS startup installation, and local-only processing before paid adapters. These are scoped engineering choices, not removals from the release contract.

## Spending ledger

The authorized cumulative external-spend ceiling is **USD 10**, excluding the user's coding-session costs. Settled spend: **USD 0**. Outstanding commitments: **USD 0**. Available: **USD 10**.

No paid service is enabled. Local builds, synthetic fixtures, public documentation, and free package downloads are the initial workflow. Do not enable recurring billing, paid hosted CI, paid inference, or cloud compute implicitly. A future paid experiment needs a bounded allocation, reservation before dispatch, reconciliation, and preservation of uncertain charges. The development ceiling is separate from the application's default-disabled provider budgets.

## Evidence and continuation

Planning baseline: commit `513b59c`, private `blisspixel/sigy`, Apache 2.0. There was no application or test suite at that checkpoint. The desktop available for first local verification is Windows x86_64 with an AMD Ryzen 7 7840U and approximately 64 GiB RAM. Other operating systems, small-host capacity, physical radios, and language quality are not qualified by a Windows build.

The current suite has 44 passing tests: 9 domain, 10 transactional ledger, 9 capture journal, 4 library, 4 controller protocol, and 8 real CLI process tests. `./scripts/verify.ps1` passed, including native hashes, formatting, tests, warnings-denied Clippy, build and dependency auditing. `cargo build --release --workspace --locked` passed. The audit covered 59 resolved dependencies against 1,251 loaded advisories; `cargo tree --locked --target all --duplicates` reported no duplicate versions. These are local results, not native-code security or platform certification. Runtime status and its assertion confirm SQLite 3.53.4.

An optimized-binary smoke run initialized a fresh library, started a detached controller, changed an exact lifetime budget through a separate client, reconnected, stopped and reopened the library with the value intact. Capture/provider dispatch remained unavailable throughout. Documentation validation passed for 52 first-party Markdown files and 240 local links/anchors, with requirement-register and writing checks. No external spend was incurred.

The ledger currently enforces lifetime scopes with global admission always included. It has no provider dispatch or automatic period reset. Daily/monthly windows, provider quote bounds, physical resource admission, source configuration, media workers, segment finalization and retention remain future slices. Capture limits currently bound catalog admission to 256 pending jobs and two active lifecycle slots; finite byte limits are stored but not yet enforced against media writes. Source revision keys are internal references awaiting the source catalog. Capture completion is rejected until verified media publication exists. The library lock is not a claim of OS service installation or complete filesystem sandboxing.

The controller uses Interprocess 2.4.4 and Tokio 1.53.1. Windows process creation uses the kernel-only winsafe 0.0.29 path to disable handle inheritance. See [controller decision](../decisions/0002-local-controller.md) and [capture journal decision](../decisions/0003-capture-journal.md). Windows directory ACL qualification and real Unix execution remain open. IPC v2 rejects mismatched clients; stop the old controller before replacing its binary. New catalogs use schema v2; older v1 catalogs migrate atomically, with failure rollback and preserved liability tests.

The user reported a host crash. No cause has been established. Builds are limited to two jobs; the shared verification script runs at most two tests per binary concurrently. CLI fixture processes have deadlines and bounded output reads. Capture journal fault tests establish transactional recovery, not media survival through physical power loss.

Next bounded slice: add versioned source configurations and a first finite internet-radio acquisition through the existing capture lifecycle. Research and qualify the network/media boundary before adding dependencies. Require destination/redirect policy, explicit byte/time/storage admission, owned-worker cleanup, verified segment publication, and deterministic local stream/restart fixtures before exposing a recording command. Then connect station discovery and the terminal explorer. No new scheduler, catalog, ledger or retry implementation should be introduced in an adapter.

Keep completed work, exact verification commands, remaining limitations, and the next bounded task here. Maintain architecture decisions under `docs/decisions/`; keep disposable experiments and raw local receipts under ignored `.agents/`. No release or deployment is authorized by this work record.
