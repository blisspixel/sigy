# Foundation language probe

Run: 2026-09-20 on Windows x86_64, AMD Ryzen 7 7840U, approximately 64 GiB RAM. These are small reproducible viability probes, not a capacity profile or a statistically controlled performance study.

Both programs use only their standard libraries and verify:

- Sixteen workers competing for 10,000 accounting units at 13 units per admission: exactly 769 accepted, 3 remaining.
- A 64-slot queue carrying 20,000 newly allocated 4,096-byte blocks: exactly 81,920,000 bytes and the expected checksum.
- Cancellation of a producer attempting to send into a full queue.

Verified commands from the repository root:

```powershell
New-Item -ItemType Directory -Path .agents/evaluation -Force
rustc +1.98.1 --edition 2024 -O -D warnings research/experiments/foundation/rust.rs -o .agents/evaluation/rust-foundation.exe
$env:GOTOOLCHAIN = 'go1.27.1'
go build -o .agents/evaluation/go-foundation.exe research/experiments/foundation/go.go
go vet research/experiments/foundation/go.go
go run -race research/experiments/foundation/go.go
1..3 | ForEach-Object { & .agents/evaluation/rust-foundation.exe }
1..3 | ForEach-Object { & .agents/evaluation/go-foundation.exe }
```

All invariants passed. Non-instrumented queue measurements in microseconds:

| Run | Rust 1.98.1 | Go 1.27.1 |
| --- | ---: | ---: |
| 1 | 15,910 | 34,950 |
| 2 | 15,724 | 33,767 |
| 3 | 12,619 | 35,221 |

The Go race-instrumented run passed and reported 455,830 microseconds. That timing is not comparable to an optimized non-instrumented run. Scheduling, allocation behavior, compiler vectorization, warm-up, and uncontrolled desktop load affect these short measurements. There is no inference workload, storage durability, network source, live audio, memory measurement, or physical radio in this probe. It cannot select a language on its own.

The [foundation decision](../../../docs/decisions/0001-rust-foundation.md) records the engineering interpretation and remaining qualification. These programs are research artifacts, not application dependencies.
