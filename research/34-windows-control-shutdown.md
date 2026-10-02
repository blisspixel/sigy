# Windows control-client shutdown investigation

Reviewed: 2026-10-01, with a 2026-10-02 reproduction and structural repair. Scope: the existing local source-registration fixture, repeated real client processes and pinned Windows transport/runtime sources. The corrupting write remains unidentified; this establishes no platform qualification or roadmap stage exit.

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

## 2026-10-02 reproduction

A private driver ran real `sigy` client processes against a foreground service on fresh local libraries, with no network or paid work. One mode repeated the stop path in cycles: library setup, service start, nine status, registration and conflicting-registration clients in parallel groups of three, then `service stop`. The other kept six `service status` clients running at once and stopped and restarted the service every 600 clients. The host ran at 100% CPU throughout because other builds shared it, so these are counts under uncontrolled load, not measured rates. A conflicting registration that commits before the matching one makes the matching client exit with status 1; these expected races are counted separately and are not failures. Cycle counts include each cycle's library setup process.

The previous code reproduced the failure four times. Each failing client had printed its complete response and then exited with `0xc0000374`.

| Build | Driver | Client processes | Heap failures |
| --- | --- | ---: | ---: |
| Preserved instrumented executable from the collection checkpoint | stop-path cycles | 2,706 | 1 (status) |
| Ordinary debug build of `f5c891d` | stop-path cycles | 2,508 | 1 (registration) |
| Preserved instrumented executable | parallel status | 15,278 | 2 (status) |

That is 4 failures in 20,492 client processes, about one in 5,100, in both ordinary and instrumented builds. All 500 service processes exited cleanly. The host's crash-dump folder gained no file, so these reproductions add exit statuses and output only, not stacks.

One hypothesis was tested and not supported. On this host `KernelBase!FlushFileBuffers`, disassembled offline from matching cached symbols, returns success for any non-negative status. If the I/O manager returned `STATUS_PENDING` for a flush on an overlapped pipe, the linger thread's stack status block would be written after its frame returned. A safe fixture instead observed the real Interprocess flush on an overlapped pipe wait for its unread peer until a 3-second bound expired, so that path was not established here.

## 2026-10-02 structural repair

[Local stream close](../docs/decisions/0071-local-stream-close.md) owns every control and listen stream so that dropping it clears the dirty-pipe flag through the supported API and never starts the detached linger thread. The client closes after a complete response. The service waits for the client to close within the existing absolute deadline and then drops its end. Wire format, catalog schema and local IPC remain v43; no operation is retried or replayed.

Fourteen transport fixtures cover complete, stalled, partial, never-closing, early-closing, trailing-byte, cancelled, pending-read, late-reader and listen-pipe cases. Two Windows fixtures use a synchronous client end, which posts no read of its own, so unread output stays in the pipe buffer. With the previous dirty drop restored through a temporary switch, 7 of the 14 failed: both synchronous fixtures, the client success-path regression, a dropped writer and a cancelled reader observed by an asynchronous peer, and both 3 MiB late-reader fixtures, which did not reach end of stream within 20 seconds. With the supported disarm all 14 pass.

Repeated runs on the repaired code under the same kind of load:

| Build | Driver | Client processes | Heap failures |
| --- | --- | ---: | ---: |
| Ordinary repaired build | stop-path cycles | 3,762 | 0 |
| Ordinary repaired build | parallel status | 11,354 | 0 |
| Coverage-instrumented repaired build | parallel status | 9,108 | 0 |
| Base client against repaired service | parallel status | 9,293 | 0 |

The repaired clients ran 24,224 processes without a failure, and all 393 repaired service processes, including the mixed arm, exited cleanly. At the earlier rate about 4.7 failures would be expected, and zero has a probability near 0.9%. Given four failures across both builds, all four landing in the earlier build has a probability near 4.4%. Load differed between arms and was not controlled, so this supports the repair but does not prove the cause. The mixed arm also stayed clean where about 1.8 failures would be expected; it neither confirms nor excludes a client-only origin.

Sixty interleaved sequential `service status` calls per build at 100% host load took a median of 1,136 ms before and 745 ms after, with 90th percentiles of 2,426 ms and 1,756 ms. No client latency regression was observed; the spread is load noise, not a speed claim.

The cause remains unproven. The defect stays tracked until repeated runs on later builds stay clean, and a recurrence must be preserved with its exit status and output before any further change. Driver scripts, summaries, binaries with their hashes and the stray build-script profile inventory are private under `.agents/heap-shutdown/`. External spend and reserved liability remain USD 0.
