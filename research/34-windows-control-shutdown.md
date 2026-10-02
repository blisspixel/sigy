# Windows control-client shutdown investigation

Reviewed: 2026-10-01. Scope: the existing local source-registration fixture and pinned Windows transport/runtime sources. This investigation establishes no repair, platform qualification or roadmap stage exit.

## Recorded failures

The first task-collection coverage attempt printed a complete successful `service stop` response, then the client exited with `0xc0000374` (`STATUS_HEAP_CORRUPTION`). Its failed log remains preserved in [active work](../docs/development/progress.md#tracked-intermittent-test-issues).

An existing local crash dump was subsequently found for that exact client. Accurate operating-system symbols identify `HEAP_FAILURE_FREELISTS_CORRUPTION`, detected while a runtime worker exits through `LdrpFreeTls`, `RtlFreeHeap` and free-block coalescing. Main is waiting for runtime shutdown; a detached pipe linger thread is concurrently freeing an allocation.

A second existing dump is from an earlier ordinary executable, outside coverage instrumentation and before task collection. It records the same heap-failure category, detected during process-exit allocation, with a pipe linger thread still freeing an allocation. Thus instrumentation and the collection increment are not necessary conditions for the observed failure. Concurrent deallocation alone does not identify the corrupting write.

The dumps lack complete heap contents. Original application symbols do not match the retained later executable; approximate application-offset mappings are not accepted as proven source frames. Raw dumps, symbols, logs, executable copies and source receipts remain private under ignored `.agents/shutdown-fix/`.

## Source review

The pinned interprocess 2.4.4 local-socket wrapper implements flush and shutdown as no-ops. Its Windows dirty-pipe drop can transfer the native pipe to a detached linger thread. The supported underlying `assume_flushed` API prevents that transfer for an unsplit, un-cloned pipe. Disarming abandons any output not yet consumed. These properties are documented in [the local socket API](https://docs.rs/interprocess/2.4.4/interprocess/local_socket/tokio/enum.Stream.html) and [the Windows pipe API](https://docs.rs/interprocess/2.4.4/x86_64-pc-windows-msvc/interprocess/os/windows/named_pipe/tokio/struct.PipeStream.html), and were checked against the pinned cached sources.

An actual server-side [FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers) can wait for peer consumption. Moving it to a [blocking task](https://docs.rs/tokio/1.53.1/tokio/task/fn.spawn_blocking.html) does not make it cancellable. It is unsuitable as an unbounded cleanup operation on a peer that may stop reading.

Independent reviews found no proven wrong-layout or double-drop defect in the reachable boxed linger path. Pinned Mio retains pending-read storage through completion ownership. [CancelIoEx](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-cancelioex) requests cancellation without waiting for completion; dropping an application stream does not prove every native completion has settled.

The [exact compiler source](https://github.com/rust-lang/rust/blob/48a229ceaefd4985c50990b14116b6d856af0985/compiler/rustc_llvm/llvm-wrapper/PassWrapper.cpp#L754-L764) enables atomic coverage counters. The examined [LLVM Windows profile runtime](https://github.com/llvm/llvm-project/blob/llvmorg-22.1.8/compiler-rt/lib/profile/InstrProfilingPlatformWindows.c) uses static linker sections, while [exit registration](https://github.com/llvm/llvm-project/blob/llvmorg-22.1.8/compiler-rt/lib/profile/InstrProfilingUtil.c#L574-L578) uses process `atexit`. No profile-owned worker TLS cleanup mechanism was established. Instrumentation may affect timing or layout; neither that possibility nor detached cleanup is a demonstrated cause.

## Bounded followup and stopping point

The exact fixture passed 37 further debugger-controlled runs with unchanged assertions and two test threads. One additional attempt was deliberately interrupted by the profile storage guard and is excluded from the pass count. The observed aggregate profile peak was 504,081,984 bytes, below the 512 MiB cap; no owned child survived cleanup. No new product crash or dump was reproduced.

Only the final passing run confirmed the corrected debugger startup hook. The driver showed heap checking enabled, but child startup inspection did not establish enhanced child heap checks. Earlier invocations omitted the explicit debugger telemetry opt-out flag; later invocations included it. No diagnostic upload was requested, and prior debugger telemetry behavior was not established. These are diagnostic limitations, not passing qualification evidence.

**Proposed repair direction:** one finite response receipt and orderly close, under the existing absolute deadline, combined with supported abandonment on error or cancellation. This could remove detached dirty-pipe cleanup without replaying a committed operation. It still requires complete-response, hostile-peer, pending-I/O, cancellation, version-compatibility and fault-origin evidence.

The user requested a stopping point for the night. An unverified transport draft was removed from the working tree and preserved privately; catalog schema and local IPC remain v43. No speculative workaround, retry, test exclusion, dependency change or claimed heap fix was published. Resume with a bounded origin-focused reproduction before choosing a repair. External spend and reserved liability remain USD 0.

## Closeout verification

The final local `cargo verify` passed 736 test executions, with 16 native fixtures reserved for their separate gate. Formatting, warnings-denied Clippy, native-source hashes, build and the dependency audit passed; the audit checked 312 dependencies against 1,279 advisories. The local log is `.agents/shutdown-fix/wrap/verify.log`, SHA-256 `859346f0a7f7f9ec3db570f84650984930fe2ab9d05f1c2844f588cf7d2568fe`.

Runtime sources, manifests, lockfile, native sources, installer scripts and CI configuration were compared against the preceding verified collection commit and are unchanged. Its [complete coverage and native results](experiments/task-collection-2026-10-01.md#integrated-local-results) remain the runtime evidence; coverage was not recollected for this documentation-only checkpoint. Local Markdown links, anchors, register continuity and whitespace passed. Publication requires passing Windows CI for the exact documentation commit, preserves older source tags and distributes no application binaries or models.
