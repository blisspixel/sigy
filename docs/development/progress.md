# Implementation progress

Updated: 2026-09-21. Status: active evaluation and implementation, authorized after the planning checkpoint.

## Current objective

Build the planned product in sound, verifiable increments. Keep Sigy as the working name. The complete first release still requires radio discovery, recordings/DVR, live multilingual translation, and bounded topic monitoring through the CLI and optional TUI. An internal foundation milestone is not a release.

## Work sequence

| Work | State | Acceptance evidence |
| --- | --- | --- |
| Foundation technology decision | Selected for initial development | Rust 1.98.1 and SQLite 3.53.4; reproducible Rust/Go probes, current stable dependency review, scoped decision and explicit platform limitations |
| Domain and durable catalog | Initial ledger implemented and tested | Exact amounts, capture state machine, multi-scope atomic reservations, replay protection, restart uncertainty, overcharge freeze, rollback faults, exclusive library ownership, foreign/future database rejection |
| Persistent service and CLI | Local Windows controller implemented and tested | Foreground/detached operation, reconnecting maintenance commands, explicit stop, malformed/oversized/versioned IPC, peer process identity, output-pipe closure, restart and single-owner tests; native Unix and startup installation remain pending |
| Durable capture journal | Internal lifecycle API implemented and tested | Schema v2 migration, finite intent, idempotency, atomic admission and journal writes, stale-worker rejection, interrupted recovery, bounded history and corruption checks; no source worker or media completion yet |
| Source authority and HTTP transport | CLI catalog and internal transport implemented with local tests | Immutable schema v3 revisions, explicit exact-IP grants, pre-connection DNS checks, shared byte/time/admission limits, TLS rejection, malformed/truncated/stalled local HTTP fixtures; no executable capture jobs or verified media publication |
| Internet radio and recording | Integration pending | Station discovery, bounded worker shutdown, physical storage admission, codec validation, durable segments and failure/restart fixtures |
| Terminal explorer and DVR | Planned | CLI parity, rendered terminal evidence, safe multilingual text, map/globe, retention and timeline tests |
| Local processing and monitoring | Planned | Typed provider boundaries, real local smoke results, per-language limits, grounded outputs and policy tests |

Follow the detailed experiment and assurance registers in `docs/planning/`. Initial implementation defaults can bound open questions without claiming universal support: captions before synthesized speech, explicit finite capture limits, foreground service operation before OS startup installation, and local-only processing before paid adapters. These are scoped engineering choices, not removals from the release contract.

## Spending ledger

The authorized cumulative external-spend ceiling is **USD 10**, excluding the user's coding-session costs. Settled spend: **USD 0**. Outstanding commitments: **USD 0**. Available: **USD 10**.

No paid service is enabled. Local builds, synthetic fixtures, public documentation, and free package downloads are the initial workflow. Do not enable recurring billing, paid hosted CI, paid inference, or cloud compute implicitly. A future paid experiment needs a bounded allocation, reservation before dispatch, reconciliation, and preservation of uncertain charges. The development ceiling is separate from the application's default-disabled provider budgets.

## Evidence and continuation

Planning baseline: commit `513b59c`, private `blisspixel/sigy`, Apache 2.0. There was no application or test suite at that checkpoint. The desktop available for first local verification is Windows x86_64 with an AMD Ryzen 7 7840U and approximately 64 GiB RAM. Other operating systems, small-host capacity, physical radios, and language quality are not qualified by a Windows build.

The prior foundation checkpoint is commit `c2311ca`. The current suite has 65 passing tests: 9 domain, 10 transactional ledger, 9 capture journal, 4 library, 4 controller protocol, 7 source configuration/catalog, 12 HTTP/resolver and 10 real CLI process tests. `./scripts/verify.ps1` passed with native hashes, formatting, tests, warnings-denied Clippy, build and advisory auditing. The audit checked 182 dependency entries against 1,258 loaded advisories without findings. The all-target duplicate-version review is recorded in acquisition research. These are local results, not native-code security or platform certification.

`cargo build --release --workspace --locked` passed. An optimized-binary smoke initialized a fresh schema v3 library, started a detached controller, registered public and explicitly pinned local source revisions, inspected a Unicode label and paginated list, stopped the controller and reopened both revisions intact. Human output was inspected. SQLite remained 3.53.4 and capture/provider dispatch remained unavailable. Documentation validation passed for 55 first-party Markdown files and 250 local links/anchors, with requirement-register and writing checks. External spend and outstanding commitments remain USD 0.

The HTTP adapter's successful local transfer fixture deliberately carries non-audio bytes labeled as audio. That proves the transport receipt cannot stand in for media validation. Other fixtures cover exact byte caps, chunked transfer, deadlines during headers/body/sink writes, cancellation and shared capacity, redirects, loopback DNS results, oversized headers, unsafe response types, TLS certificate rejection and sink errors. No public stream or provider inference was contacted.

The ledger currently enforces lifetime scopes with global admission always included. It has no provider dispatch or automatic period reset. Daily/monthly windows, provider quote bounds, physical resource admission, media workers, segment finalization and retention remain future slices. Capture limits currently bound catalog admission to 256 pending jobs and two active lifecycle slots; stored capture limits are not yet connected to media writes. Registered source revisions now exist, but legacy internal capture references do not become executable automatically. Capture completion is rejected until verified media publication exists. The library lock is not a claim of OS service installation or complete filesystem sandboxing.

The controller uses Interprocess 2.4.4 and Tokio 1.53.1. Windows process creation uses the kernel-only winsafe 0.0.29 path to disable handle inheritance. See [controller decision](../decisions/0002-local-controller.md), [capture journal decision](../decisions/0003-capture-journal.md) and [source/HTTP decision](../decisions/0004-source-authority-and-http.md). Windows directory ACL qualification and real Unix execution remain open. IPC v3 rejects mismatched clients; stop the old controller before replacing its binary. New catalogs use schema v3; older v1/v2 catalogs migrate atomically, with failure rollback and preserved accounting tests.

Reqwest 0.13.5 is the first canonical HTTP adapter. TLS uses explicit bundled roots and offline WebPKI verification to avoid certificate-driven network retrieval outside source policy. Native cryptographic dependencies, root-update obligations and upstream version duplication are recorded in [acquisition research](../../research/25-http-acquisition.md). This is a reviewed dependency increase, not a dependency-free or pure-Rust binary claim.

The user reported a host crash. No cause has been established. Builds are limited to two jobs; the shared verification script runs at most two tests per binary concurrently. CLI fixture processes have deadlines and bounded output reads. Capture journal fault tests establish transactional recovery, not media survival through physical power loss.

Next bounded slice: qualify cancellable or supervised DNS and worker shutdown, then integrate finite acquisition with physical storage reservations, safe file ownership, media validation and publication through the existing capture lifecycle. System DNS cancellation currently retains its admission slot correctly, but a stuck OS lookup can delay runtime shutdown. Do not wire unattended capture to that unresolved lifecycle. Add source-permission revocation, original-byte checksums, timing/completeness and orphan reconciliation, with process-kill and disk-pressure fixtures before exposing a recording command. Then connect station discovery and the terminal explorer. No new scheduler, catalog, ledger or retry implementation should be introduced in an adapter.

Keep completed work, exact verification commands, remaining limitations, and the next bounded task here. Maintain architecture decisions under `docs/decisions/`; keep disposable experiments and raw local receipts under ignored `.agents/`. No release or deployment is authorized by this work record.
