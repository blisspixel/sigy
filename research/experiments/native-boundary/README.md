# Native process boundary probe

Run: 2026-09-22 on Windows 11 Pro build 26200, AMD Ryzen 7 7840U, Rust 1.98.1. This package is excluded from the Sigy workspace. It is a reproducible candidate experiment, not an application dependency or a selected inference worker. It runs only its own executable and opens no network connection or retained recording.

From the repository root:

```text
cargo fmt --manifest-path research/experiments/native-boundary/Cargo.toml --check
cargo clippy --locked --manifest-path research/experiments/native-boundary/Cargo.toml -- -D warnings
cargo audit --file research/experiments/native-boundary/Cargo.lock
cargo run --locked --quiet --manifest-path research/experiments/native-boundary/Cargo.toml
```

The pinned candidate is ProcessKit 3.3.4 with its `limits` feature. Its declared MIT license and the 35-package lockfile were reviewed; the advisory scan on 2026-09-22 found no listed vulnerability. This probe uses the safe group API and creates a Windows Job Object. It tests a deliberately small child tree; no model or corpus is downloaded.

The preserved [run receipt](results/2026-09-22.txt) reported:

| Check | Observed result |
| --- | --- |
| Invalid process cap of zero | Group creation refused before a child launch |
| One-process cap | Second child launch refused; the first child was reaped and the group reached zero active members |
| Root exits with a grandchild | One member remained; group termination reached zero active members within the two-second check |
| 32 MiB job memory cap | Child reported an explicit allocation refusal; job peak committed memory was 34,430,976 bytes |
| 0.25-core CPU quota | A two-second busy child accumulated 0.65625 seconds of CPU time; the uncapped control accumulated 1.984375 seconds |
| Output without a newline | Reader detected the 64 KiB limit at byte 65,537, terminated the child, reaped it, and confirmed zero active members |

The memory peak exceeded the configured 33,554,432-byte limit by 876,544 bytes. This check demonstrates a refused allocation, not an exact peak-memory ceiling. The CPU figures are one local comparison under uncontrolled host load, not a calibrated rate guarantee. Windows job committed memory excludes some mapped and GPU/shared allocations. The output check caps the bytes read by this fixture; an application worker still needs its own bounded output and publication path.

ProcessKit's raw group spawn overwrites Windows creation flags, including `CREATE_NO_WINDOW`; this fixture does not qualify the application's window behavior. The candidate also has a suspended-create to job-assignment gap on abrupt owner death, and a job does not deny network access. Cancellation, owner death, nested jobs, durable input-lease release, and network denial remain separate gates before model execution. The [research note](../../30-language-pipeline-evaluation.md#windows-boundary-candidate-2026-09-22) records the upstream source and alternatives.
