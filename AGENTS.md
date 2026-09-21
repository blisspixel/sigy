# Repository instructions

## Product and current phase

Sigy is the working name for a local-first, multilingual signals discovery and analysis platform. Read [README.md](README.md), [INTENT.md](INTENT.md), [ROADMAP.md](ROADMAP.md), and the [planning index](docs/planning/README.md) before consequential changes.

The current phase is documentation and research only. Do not create application code, prototypes, dependency manifests, installed services, or model downloads until the user advances the phase. Python is excluded. Rust is the leading recommendation for evaluation, not a selected stack. Follow the evidence gates in [Delivery and decisions](docs/planning/05-delivery-and-decisions.md). Naming is also unresolved; do not turn a candidate into a rename without a decision.

Reinspect the working tree before each task. On 2026-09-20 it contains planning documents and the Apache 2.0 license, with no implementation, build configuration, tests, or CI. Git is initialized on `main`, with `origin` pointing to the private `blisspixel/sigy` repository. Hosting uses the working name and does not settle product naming. Update this baseline when it changes. Distinguish confirmed intent, proposed design, implemented behavior, tested behavior, released behavior, and operationally validated behavior.

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

There are currently no project install, build, lint, or test commands. Do not invent them or present proposed CLI examples as executable functionality. A verified discovery command is `rg --files --hidden -g '!.git/**' -g '!.agents/**'`. For documentation changes, check relative links and anchors, requirement/decision identifiers, status consistency, source support, and writing rules. Describe exactly what was checked.

After implementation is authorized, derive verification commands from actual manifests, configuration, scripts, and tool help before documenting them. Use the selected ecosystem's strong compiler, type, lint, and security checks. Verify the relevant behavior, inspect failures, fix causes, rerun affected checks, and self-review. Never weaken checks, assertions, schemas, or error handling merely to pass. Use crash, concurrency, cost, native-boundary, and real platform evidence where the change requires it. Performance claims need measurements; rendered interfaces need inspection.

Keep one coherent implementation of shared behavior. Add enforceable invariants when recurring mistakes expose a structural weakness. Scorecard is supporting supply-chain evidence, not proof of product correctness. Do not fabricate reviews, tests, support claims, or maturity to improve a score.

## Continuity and consequences

Keep disposable maps, diagnostics, scratch notes, and local receipts in gitignored `.agents/`; never store credentials there. Promote durable conclusions to canonical documentation, decisions, tests, or bounded tickets. Add structural indexes or coordination machinery only when they materially help, and keep them synchronized. This file is the canonical shared instruction source; add thin tool-specific pointers only when needed.

Update affected project state after meaningful work. Do not infer permission to spend, publish, send messages, delete user data, or change production systems from a routine local editing task. Respect authorization already given. Refining these instructions alone does not authorize Git initialization, commits, pushes, releases, or deployment. Instruction Markdown does not enforce runtime security or spending boundaries.

## Writing and attribution

The only permitted author and public attribution identity is Nick Seal <32712898+blisspixel@users.noreply.github.com>, GitHub username blisspixel. Do not add Codex, Claude, Anthropic, OpenAI, ChatGPT, Copilot, other assistant, model, vendor, tool, or coauthor credits to public repository content. This includes commits, PRs, releases, tags, documentation, images, and UI text. Do not add attribution trailers, signatures, footers, badges, generated-by wording, negative attribution disclaimers, or Co-Authored-By trailers. Instruction files may name prohibited credits to define this policy. Technical references to dependencies and providers are not authorship credits.

Preserve required third-party copyright, license, and NOTICE content. Keep Apache 2.0 intact. Use concise professional writing without emojis, em dashes, or en dashes. Keep documentation current and tidy. Do not introduce names, links, or copied assets from private inspiration references into public project content.
