# Language and stack research

Reviewed: 2026-09-20. Status: original evidence used for the subsequent [Rust foundation decision](../docs/decisions/0001-rust-foundation.md). The comparison below predates that decision. Python is excluded by product direction.

## Rust evidence

Rust's ownership rules enforce memory-management constraints at compile time. This offers a relevant foundation for buffer ownership and explicit resource lifetimes. It does not establish freedom from deadlocks, unbounded allocation, logic errors, or failures inside native dependencies. [Ownership](https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html).

Rust's foreign-function interface requires explicit attention to unsafe boundaries, memory ownership, callbacks, and unwinding. A wrapper around a native decoder or driver still needs a documented safety contract. [FFI guidance](https://doc.rust-lang.org/nomicon/ffi.html).

Tokio documents that blocking tasks already running cannot be aborted by aborting their task handle. It also calls for explicit bounds on CPU work. Cancellation and worker isolation therefore require design even in a memory-safe systems language. [Blocking tasks](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html), [cancellation safety](https://docs.rs/tokio/latest/tokio/macro.select.html#cancellation-safety).

Rust documents target-support tiers, and Cargo describes dependency resolution and lockfiles. These provide inputs to a build/support policy, not a guarantee that every native library cross-compiles. [Platform support](https://doc.rust-lang.org/rustc/platform-support.html), [dependency resolution](https://doc.rust-lang.org/cargo/reference/resolver.html).

## Go evidence

Go's garbage collector runs much of its work concurrently and exposes memory/CPU tradeoffs. Its memory limit is soft and does not account for all memory outside the runtime, including allocations managed by native code. This requires whole-process and worker accounting for media/inference workloads. It is not evidence that Go cannot meet radio latency requirements. [GC guide](https://go.dev/doc/gc-guide).

cgo supports C integration with pointer and calling rules. Cross-compiling cgo requires an appropriate C cross-compiler. The simplicity of a pure-Go executable cannot be assumed after embedding native media or hardware libraries. [cgo reference](https://pkg.go.dev/cmd/cgo).

Go's compatibility policy is a concrete long-term-maintenance strength within its stated scope. Modules provide dependency/version handling. Both still require dependency review and cross-version testing. [Compatibility policy](https://go.dev/doc/go1compat), [modules reference](https://go.dev/ref/mod).

Go provides fuzzing and a race detector. Rust's ecosystem provides fuzzing tools as well. Tool availability is useful, while actual assurance depends on tests covering the important parser and concurrency boundaries. [Go fuzzing](https://go.dev/doc/security/fuzz/), [Go race detector](https://go.dev/doc/articles/race_detector), [Rust fuzzing tools](https://github.com/rust-fuzz/cargo-fuzz).

## Candidate terminal ecosystems

[Ratatui](https://ratatui.rs/) with [Crossterm](https://github.com/crossterm-rs/crossterm) and [Bubble Tea](https://github.com/charmbracelet/bubbletea) are viable research subjects for the proposed TUI. Evaluate current major versions, maintenance, testing interfaces, Unicode behavior, accessibility, focus management, and large-data rendering rather than judging screenshots alone.

## Engineering interpretation

Rust deserves close evaluation when explicit memory ownership, native integration, and eventual high-rate sample handling carry substantial weight. Go deserves close evaluation when the application primarily supervises external workers and its dominant complexity is service coordination, network operations, and operational maintenance.

Both can implement reliable orchestration. Neither makes inference faster merely by hosting the same external model runtime. Neither turns a desktop operating system into a hard real-time platform. The decisive question is where Sigy's actual complexity and hot data paths live.

An initial Rust/Go hybrid is not assumed. Two application languages introduce extra build and interface costs and need evidence of a material benefit. Existing native dependencies are a separate consideration from selecting multiple application languages.

## Evidence still needed

Measure comparable bounded designs for service supervision, event subscription, media handoff, native callbacks, resource accounting, and TUI interaction. Use identical native/model workers when evaluating language overhead. Separate application overhead from codec and model costs.

Review maintainer ability to diagnose each candidate, dependency churn, unsafe/native scope, build complexity, and testability. An implementation that is harder to understand needs a measurable benefit to justify that maintenance cost.

No benchmark has been run in this documentation phase. The [trade study](../docs/planning/06-language-and-stack-trade-study.md) defines selection criteria and future experiments without assigning invented scores.

## Near future

Recheck supported compiler versions, terminal-library APIs, async cancellation guidance, and native binding health at selection. Prefer stable contracts around changing backends. Avoid speculative dependence on unreleased language features.
