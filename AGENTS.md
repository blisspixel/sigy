# Repository instructions

## Product and current phase

Sigy is a local-first, multilingual signals discovery and analysis platform. Read [README.md](README.md), [INTENT.md](INTENT.md), [ROADMAP.md](ROADMAP.md), and the [planning index](docs/planning/README.md) before consequential changes.

The user advanced the project to evaluation and implementation on 2026-09-20. Build in verified increments from the existing design; do not claim unfinished gates or release support. Rust 1.98.1 is selected for the foundation; Python is excluded. The user chose to keep Sigy on 2026-09-21; further naming exploration is deferred. Follow [active work](docs/development/progress.md), the [foundation decision](docs/decisions/0001-rust-foundation.md), and the evidence gates in [Delivery and decisions](docs/planning/05-delivery-and-decisions.md).

Reinspect the working tree before each task. The Rust workspace has domain, catalog/ledger, capture journal, source registration, local controller, finite audio recording, retention, metadata export and bounded Radio Browser refresh/search with local Windows tests. Podcasts, integrated playback, TUI and model dispatch remain unimplemented. There is no release or qualified platform matrix. `main` tracks the private `blisspixel/sigy` repository. Distinguish confirmed intent, proposed design, implemented behavior, tested behavior, released behavior, and operationally validated behavior.

## Context and canonical homes

| Task | Read next |
| --- | --- |
| Service, persistence, capture, recovery | [Architecture and data](docs/planning/02-architecture-and-data.md) |
| Providers, network processing, spending | [Providers and cost policy](docs/planning/07-providers-and-cost-policy.md) |
| New sources, decoders, cryptography | [Signal extensions](docs/planning/08-signal-extensions-and-workbench.md) |
| Input trust, credentials, distribution | [Security and release design](docs/planning/09-security-privacy-and-release.md) |
| Classification, queues, findings, statistics | [Analysis and knowledge](docs/planning/10-analysis-and-knowledge.md) |
| CLI/TUI, catalog, geography, DVR | [Explorer and DVR](docs/planning/11-radio-explorer-and-dvr.md) |
| Layout, dependencies, verification, Scorecard | [Repository engineering](docs/planning/12-repository-and-engineering.md) |
| Acceptance evidence | [Assurance and validation](docs/planning/04-assurance-and-validation.md) |

Confirmed requirements live in the planning index; open decisions and work packages live in Delivery and decisions. Dated external evidence belongs in [research/](research/README.md). Inspect relevant source, tests, manifests, CI, and history once they exist; they outrank stale descriptions of implementation. Ask about consequential unresolved intent, and resolve routine details through these contracts.

## Preserve these boundaries

- The first complete release includes radio exploration, recordings, live translation, and bounded autonomous topic monitoring. A complete CLI and optional TUI share service operations. The persistent service owns jobs; client exit must not stop them. Geographic exploration and DVR belong to the initial terminal experience.
- Most expected speech and music are non-English. Preserve original scripts and per-span language evidence, including mixed and unknown states. English is the primary translation target. Claim language quality and simultaneous capacity only for measured profiles.
- Local processing is the default. Paid use needs explicitly configured finite limits. Enforce exact accounting and atomic worst-case reservations before dispatch, including retries and fallback. Retain unresolved charges across crashes and billing periods. Missing bounds fail closed; overload must not trigger paid processing automatically.
- Models and retrieved content cannot grant permissions or change budgets. Models propose actions; validated service policy executes them. Treat network data, files, database records, model output, and terminal text as untrusted boundaries. Keep semantic uncertainty explicit rather than disguising it as arbitrary deterministic scoring.
- Keep capture independent of analysis and rendering, with bounded buffers, queues, resource admission, and recovery. Preserve originals while retained, provenance, revisions, timing, and gaps. Rankings describe measured source coverage, not worldwide popularity.
- Extend typed source/transform/provider contracts for audio, IQ, packets, telemetry, and text. Reuse the canonical scheduler, ledger, storage, configuration, logging, HTTP, and retry mechanisms. Do not create parallel infrastructure or assume every signal is audio. Follow the lawful-use scope; hardware starts with reception. Historical ciphers remain separate from modern supplied-key cryptography using maintained implementations.

## Work and verification

Research changing technical facts from primary sources before consequential choices. Record the review date, alternatives, limitations, and evidence needed. Prefer stable GA technology, intentional dependencies, and one application language unless a measured need justifies another. Review the complete dependency and distribution graph, including native libraries and model assets.

The verified local entry point is `./scripts/verify.ps1` in PowerShell. It validates the native source hashes, formatting, workspace tests, warnings-denied Clippy, build, and dependency advisories. It requires cargo-audit; do not silently skip missing checks. The underlying commands are in the script. Use the pinned toolchain and preserve `Cargo.lock`; regenerate it intentionally after manifest changes. For documentation, check links/anchors, register IDs, status consistency, sources, and writing rules. Planning CLI examples are proposals unless the actual parser implements them.

`sigy-core` has no dependencies. `sigy-service` owns the SQLite catalog, migrations, accounting, and library ownership; interfaces reuse those operations. `sigy` owns command parsing and output. Domain values validate before storage or execution. Never replace exact monetary strings/integers with floating point, drop uncertain liabilities, or dispatch on an idempotent replay of a submission transition. The single library lock must outlive the catalog connection.

Use the existing [local controller](docs/decisions/0002-local-controller.md) and [capture journal](docs/decisions/0003-capture-journal.md). Capture mutations require the exact revision/generation; recovery invalidates old workers. Keep state and journal changes in one transaction. Do not expose capture dispatch or completion before source authorization, resource enforcement, and verified media publication exist.

Source authority and HTTP acquisition live in `sigy-service::sources`; immutable revisions live in `storage::sources`. Share one acquisition instance, preserve destination checks and explicit grants, and keep asynchronous DNS cancellable. A transfer receipt is not verified media. Redirects use immutable opt-in policy, at most three hops, the original deadline and checked destinations at every hop. Preserve migration defaults and route provenance in [authorized redirects](docs/decisions/0007-authorized-redirects.md). Do not introduce implicit proxies, retries or permission expansion.

Directory adapters live in `discovery`, with refresh/cache transactions in `storage::discovery`. Reuse the controller's worker supervisor. A refresh merges a bounded page, never implies a complete catalog or starts playback, and cannot grant private-network authority to returned stations. Preserve exact-request replay and immutable source provenance. Follow [directory refresh](docs/decisions/0006-radio-discovery.md).

Recording files live behind `recordings`; `storage::dvr` owns exact reservations and publication with the capture journal. Preserve the 14-day/50 GB defaults, kept/archive protection, interrupted liabilities, and delete-before-release ordering. Follow [recording and retention](docs/decisions/0005-recording-and-retention.md). Media changes require `./scripts/verify-media.ps1` with a trusted installed FFmpeg in addition to the normal checks. Exported sidecars are snapshots, not permissions or the current retention authority. Internet radio and podcasts precede RF adapters; keep RF/IQ/packet contracts distinct from audio.

`vendor/libsqlite3-sys` is retained third-party source with a documented SQLite patch override. Preserve its notices and bytes, do not apply first-party formatting, and verify changes against [native provenance](vendor/README.md). Do not manually change checksum expectations to conceal an unexplained source change.

After implementation is authorized, derive verification commands from actual manifests, configuration, scripts, and tool help before documenting them. Use the selected ecosystem's strong compiler, type, lint, and security checks. Verify the relevant behavior, inspect failures, fix causes, rerun affected checks, and self-review. Never weaken checks, assertions, schemas, or error handling merely to pass. Use crash, concurrency, cost, native-boundary, and real platform evidence where the change requires it. Performance claims need measurements; rendered interfaces need inspection.

Keep one coherent implementation of shared behavior. Add enforceable invariants when recurring mistakes expose a structural weakness. Scorecard is supporting supply-chain evidence, not proof of product correctness. Do not fabricate reviews, tests, support claims, or maturity to improve a score.

## Continuity and consequences

Keep disposable maps, diagnostics, scratch notes, and local receipts in gitignored `.agents/`; never store credentials there. Promote durable conclusions to canonical documentation, decisions, tests, or bounded tickets. Add structural indexes or coordination machinery only when they materially help, and keep them synchronized. This file is the canonical shared instruction source; add thin tool-specific pointers only when needed.

Update affected project state after meaningful work. Do not infer permission to spend, publish, send messages, delete user data, or change production systems from a routine local editing task. Respect authorization already given. Refining these instructions alone does not authorize Git initialization, commits, pushes, releases, or deployment. Instruction Markdown does not enforce runtime security or spending boundaries.

The active implementation goal has a cumulative USD 10 external-spend ceiling, excluding the user's coding-session costs. Prefer USD 0. Do not run paid inference, paid CI, hosted compute, purchases, or recurring services without a mechanically bounded allocation and a recorded reservation in the work ledger. Existing keys and accounts are not permission to consume an unbounded balance. Keep ordinary verification local until hosted billing is qualified.

## Writing and attribution

The only permitted author and public attribution identity is Nick Seal <32712898+blisspixel@users.noreply.github.com>, GitHub username blisspixel. Do not add Codex, Claude, Anthropic, OpenAI, ChatGPT, Copilot, other assistant, model, vendor, tool, or coauthor credits to public repository content. This includes commits, PRs, releases, tags, documentation, images, and UI text. Do not add attribution trailers, signatures, footers, badges, generated-by wording, negative attribution disclaimers, or Co-Authored-By trailers. Instruction files may name prohibited credits to define this policy. Technical references to dependencies and providers are not authorship credits.

Preserve required third-party copyright, license, and NOTICE content. Keep Apache 2.0 intact. Use concise professional writing without emojis, em dashes, or en dashes. Keep documentation current and tidy. Do not introduce names, links, or copied assets from private inspiration references into public project content.
