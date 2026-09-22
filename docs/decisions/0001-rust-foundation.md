# 0001: Rust foundation with incremental qualification

Date: 2026-09-20. Status: accepted for foundation development after the user advanced the implementation phase. Release qualification remains open.

## Decision

Use Rust 1.98.1, edition 2024, for the application and service. Keep a small workspace: `sigy-core` owns validated domain invariants without dependencies, `sigy-service` owns persistence and application operations, and `sigy` composes interfaces. Existing native media/model engines can be supervised dependencies. Python remains excluded.

Use SQLite through rusqlite for the initial durable catalog. Compile the bundled engine to avoid an unqualified system SQLite dependency. Use immediate transactions for admission and reconciliation, database constraints, versioned migrations, and explicit durability settings. Do not place database or interface dependencies in the core.

The latest stable binding still bundled SQLite 3.53.2. A [documented native override](../../vendor/README.md) retains libsqlite3-sys 0.38.2 and replaces only its three SQLite amalgamation files with verified official SQLite 3.53.4 sources. A runtime version test prevents an unnoticed fallback to the older engine. Remove the override when a qualified stable upstream package contains the current engine.

Initial direct libraries are rusqlite 0.40.2, thiserror 2.0.20, clap 4.6.7, serde 1.0.229, and serde_json 1.0.151. tempfile 3.27.0 is test-only. Current stable registry metadata and licenses were checked before admission; the resolved graph is in `Cargo.lock`. Terminal, network, and async libraries are added only with the subsystem that needs them.

## Evidence and alternatives

Rust and Go both passed the same initial budget-contention, bounded media-block queue, byte-integrity, and blocked-producer cancellation probes on the available Windows desktop. Rust 1.98.1 and Go 1.27.1 were used. Go vet and the race-instrumented run passed. See the [reproducible probe](../../research/experiments/foundation/README.md).

These probes demonstrate basic local viability, not production throughput or a complete language comparison. The selection gives greater weight to explicit ownership at future native signal boundaries, exhaustive domain modeling, and one language across orchestration and native data paths. Go remains a credible alternative; it was not rejected for failing these tests. Rust's extra learning and compile-time costs are accepted for this architecture. Neither language supplies model quality or crash recovery automatically.

The first Rust workspace compiled and its nine initial domain tests passed on Windows. Formatting and warnings-denied Clippy with the `all` and `pedantic` groups passed. Library/runtime versions are supported by [Rust's release announcement](https://blog.rust-lang.org/releases/latest/), [Go release history](https://go.dev/doc/devel/release), and the actual lockfile. Rust 1.98.1 includes the vtable miscompilation fix, so 1.98.0 is not the pinned toolchain.

SQLite's [transaction semantics](https://www.sqlite.org/lang_transaction.html) and [durability controls](https://www.sqlite.org/pragma.html) support the design, but application fault and concurrency tests must supply the evidence. The [rusqlite connection API](https://docs.rs/rusqlite/0.40.2/rusqlite/struct.Connection.html) was checked before use.

The [2026-09-22 SQLite and DuckDB reassessment](../../research/08-storage-and-evidence.md#sqlite-and-duckdb-reassessment-2026-09-22) retains SQLite for the authoritative catalog. DuckDB remains a possible measured experiment for reporting over derived snapshots; no additional engine or migration is selected.

## Limits and reconsideration

The full G4 evaluation matrix is not complete. This decision authorizes one coherent foundation, with subsequent subsystem and release gates retaining unmeasured requirements. No macOS/Linux native execution, small-host profile, hardware throughput, TUI usability, multilingual model quality, or installer qualification is claimed. Do not postpone these checks until after a release claim.

Reconsider individual dependencies or process boundaries if measured resource, packaging, reliability, license, or maintenance constraints fail. Reopen the application language only for a material architecture-level failure. Do not retain a second application-language implementation after this comparison merely as a fallback.
