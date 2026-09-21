# Repository organization and engineering

Reviewed: 2026-09-20. Status: proposed organization and engineering profile. No stack is selected and no implementation directories are created. See [engineering research](../../research/20-repository-engineering.md) and the [stack trade study](06-language-and-stack-trade-study.md).

## 1. Organize around ownership

Use one repository for the application, persistent service, clients, built-in integrations, tests, and documentation. Start with a modular application and supervised specialist workers. A new source should implement an existing contract without inventing its own job system, storage library, spending policy, or UI backend.

Current canonical homes:

```text
README.md             Product entry point and truthful current status
INTENT.md             Product purpose and durable constraints
ROADMAP.md            Planned delivery and exit criteria
AGENTS.md             Concise shared development instructions
LICENSE               Apache License 2.0
.gitignore            Local disposable-state exclusions
docs/
  README.md           Design navigation
  planning/           Product contracts, designs, decisions, assurance
research/             Dated sources, alternatives, findings, unknowns
.agents/              Ignored scratch state, created only when useful
```

Keep the existing documents in place. Add decision records under `docs/decisions/` when choices are actually made, and development/operation guides when there is something real to build or operate. Superseded proposals need explicit status and links to their replacement. Do not create empty folders for every roadmap item.

## 2. Proposed Rust organization

Rust is the leading recommendation to evaluate because explicit resource ownership and native integration fit the long-term signal-processing scope. This is an engineering judgment, not a completed G4 decision. The following tree is illustrative and remains entirely uncreated:

```text
Cargo.toml                    Workspace and shared policy
Cargo.lock                    Resolved application dependencies
rust-toolchain.toml           Qualified toolchain
crates/
  sigy-core/                  Domain types, invariants, policy, ports
  sigy-service/               Jobs, workflows, adapters, persistence
    src/
      sources/                Radio first; feeds and hardware later
      processors/             Speech, translation, classifiers, decoders
      storage/                Catalog, archive, migrations, reconciliation
      providers/              Local, LAN, and paid provider adapters
      runtime/                Scheduling, admission, supervision, IPC
  sigy/                       Executable composition, CLI, optional TUI
    src/
      cli/
      tui/
      client/                 Shared service client for both interfaces
tests/                        Cross-component journeys and fault scenarios
fixtures/                     Small licensed or synthetic replay inputs
scripts/                      Actual shared maintenance/verification tools
.github/workflows/            CI and separate release workflows
```

Names remain placeholders while product naming is open. A three-package starting point is a proposal, not a minimum quota. Use modules until a separate package materially enforces dependency isolation, optional compilation, or independent delivery. Unit and package integration tests belong beside their owner; root tests are for assembled behavior. Store migrations beside the database implementation and wire-schema sources beside their protocol owner. Avoid competing schema copies or a miscellaneous `utils` package.

The dependency direction is executable composition to service/core, and service to core. The core must not import terminal rendering, provider SDKs, concrete databases, or device drivers. Adapter implementations satisfy ports owned by the domain/application boundary. CLI and TUI use the same service client and operations, including when the service is started locally. Executable packaging does not determine process isolation: one distribution can still run separate service and worker processes.

A future client-only package or shared protocol package should be extracted when a real second client or build constraint needs it. Native dependencies and third-party workers use explicit versioned contracts; an unstable Rust dynamic-library ABI is not the extension contract. High-rate sample transport and low-rate control need separate resource and timing designs. [Architecture](02-architecture-and-data.md), [extension contracts](08-signal-extensions-and-workbench.md).

If Go wins the evaluation, preserve these ownership boundaries in one module, using `cmd/sigy/` and private `internal/` packages. Do not imitate Cargo with needless Go modules. Future native/web clients are separate interfaces to the service; their frameworks remain undecided.

## 3. Provisional stack profile

| Responsibility | Leading candidate to evaluate | Boundary |
| --- | --- | --- |
| Application, service, CLI/TUI | Rust | One application language; Python excluded |
| Terminal rendering/input | Ratatui and Crossterm | Presentation consumes service projections |
| CLI parsing | clap | Reuse commands and validation across interactive/noninteractive use |
| Async I/O and supervision | Tokio | One runtime policy; CPU-heavy inference does not block control or capture |
| Catalog and durable metadata | SQLite | One embedded transactional store; binding and search approach unresolved |
| Media and speech | Maintained native engines, initially evaluated as supervised workers | FFmpeg and native speech engines are candidates, not approved defaults |
| Text/model access | Typed adapters over one shared HTTP mechanism | Ollama and OpenRouter targets; no mandatory provider SDK or agent framework |

These are candidates, not dependencies to add today. Choose exact stable versions, enabled features, supported targets, and replacement policies at G4/G5 after measurements. Rust itself does not guarantee a smaller dependency graph than Go. Count and inspect the assembled product, including codecs, drivers, native binaries, accelerator libraries, certificates, and model files.

No server database, distributed message broker, web frontend runtime, vector database, or general plugin marketplace is currently justified as a mandatory foundation. Introduce one only for a demonstrated requirement. Existing mature implementations are preferable to writing TLS, cryptography, media codecs, or standards-heavy parsers for the sake of a smaller manifest.

## 4. Dependency admission and extension discipline

For each material dependency, record its purpose, existing alternatives, maintenance and security posture, license/distribution fit, transitive/build-time impact, supported targets, and the verification needed to upgrade or replace it. Review default features and duplicate versions. Prefer one mechanism for HTTP, logging, configuration, retries, persistence, and serialization within each necessary boundary.

Keep optional hardware and specialist model integrations separable from the basic install. A configured external worker remains a dependency with a compatibility contract. Check executable identity and version; do not silently download or execute an arbitrary provider's binary. Shared-worker protocols require bounds, cancellation, health, provenance, and capability checks.

Application data belongs in documented platform data/cache/config locations outside the source tree. Do not check in recordings, downloaded models, credentials, generated executables, or large captured datasets. Small representative fixtures need redistribution permission, known hashes where appropriate, and a documented purpose. Tests should generate larger deterministic data when practical.

## 5. Verification and supply-chain roadmap

Current verification covers documentation integrity and evidence consistency. There is no build or runnable application. [AGENTS.md](../../AGENTS.md) must not advertise future commands as present commands.

After stack selection, create one local verification entry point used by CI. For Rust, qualify formatting, compiler checks, warnings-denied Clippy, useful unit/integration tests, dependency/license/advisory review, and release build checks. Scope unsafe/FFI code explicitly and use suitable fuzzing, interpreter/sanitizer, concurrency, and native-worker checks where supported. If Go is selected, qualify formatting, compilation, tests, vet, mature static analysis, vulnerability checks, and race/fuzz tests. Add exact commands only against actual configuration and supported tool versions.

Cross-platform install/start/reconnect/stop/uninstall evidence, crash recovery, storage pressure, multilingual evaluations, spending faults, and slow-terminal rendering tests remain mandatory product assurance. CI success alone cannot qualify every device, language, throughput profile, or operating system.

The working interpretation of "open source score" is OpenSSF Scorecard. Aim for strong applicable results and review individual findings periodically after implementation and repository hosting exist. No score has been run, and no numeric score is promised.

| Stage | Useful controls and evidence |
| --- | --- |
| First implementation | Locked dependencies, defined toolchain, reproducible local checks, tested platform matrix, secret scanning, dependency updates and advisories |
| Hosted collaboration | Least-privilege CI tokens, immutable action pins, isolated untrusted contributions, protected branches, required checks, genuine review appropriate to maintainer availability |
| Distribution | Actual security reporting policy, inventory/SBOM, verifiable artifacts and provenance, license notices, update and rollback validation |
| Periodic assessment | Run a qualified Scorecard release; retain date, target commit, tool identity, permissions/visibility limitations, individual findings, remediation and exceptions |

Do not add fabricated human reviews, dummy activity, meaningless fuzz targets, or unexplained exclusions for points. Some checks depend on project age, hosting access, maintainers, or detectable workflows. Record those limitations honestly. Scorecard reports supply-chain indicators; it does not prove safe spending, reliable capture, or accurate translations. Public result publication and badges are separate decisions from running an assessment. [Release design](09-security-privacy-and-release.md), [Scorecard evidence](../../research/20-repository-engineering.md).
