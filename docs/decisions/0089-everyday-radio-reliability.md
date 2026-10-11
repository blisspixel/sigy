# 0089: Everyday radio reliability

Date: 2026-10-11. Status: implemented bounded increment; integrated evidence belongs in [active work](../development/progress.md#2026-10-11-everyday-radio-reliability). Schema v51 and IPC v52 remain unchanged.

## Problem

The desktop issues exposed a Unix build error, hidden native-limit failures, panel admission using an illegal 1,024-row search, fragile directory reads and a first-use journey requiring unnecessary request/revision IDs. The user prioritized reliable daily discovery and listening. [Primary-source research](../../research/46-everyday-radio-and-linux-containment.md) informs the scope.

## Decision

- Check Unix process IDs using the actual optional PID API and scope Windows-only constants appropriately. The existing Windows verification workflow gains Linux/macOS all-target compile/lint and platform-contract jobs. These are build evidence, not platform qualification.
- Keep Linux's atomic pre-bind socket mode. On other Unix hosts, the control and byte sockets bind inside a verified owner-only 0700 library and receive mode 0600 before publication/acceptance; peer-user checks stay enforced. This avoids Interprocess's unsupported pre-bind mode operation on macOS without a process-wide umask change or a publicly traversable socket window. CI checks actual socket privacy and byte-pipe lifecycle.
- Validate control and byte-pipe address capacity with the host's standard Unix socket API before binding. Oversized paths refuse with shorter-library-path guidance. Keep full random nonces and colocated ownership; positive macOS fixtures use short private paths, while an independent long-path fixture checks refusal without a socket file.
- Preserve native-limit errors. Doctor probes an empty bounded decoder group, reports its platform reason and starts no child. New recording and live-listen admission preflight before station contact or recording reservation. Exact existing receipts retain replay without requiring a new native group. Linux's delegated adapter remains open.
- Resolve a requested panel favorite through exact station lookup and bounded linked revisions. Do not expand the 16-row search limit or relax favorite/one-revision/non-HLS policy. Optional panel IDs default to fresh requests. Failed startup settles the hint. An absent lifecycle fence stays pending rather than cancelling on closed notification channels.
- Retry only transient directory reads, at most three attempts per selected mirror, up to two discovered mirrors, within the existing 25-second whole refresh deadline. Delays are 500/1,500 ms plus bounded jitter. Explicit mirrors stay explicit. Policy, format and size refusals do not retry. Durable failures retain bounded sanitized origin, attempt count and failure class. Clicks and station acquisition gain no retry authority.
- Add optional automatic refresh IDs and `--wait`; keep explicit IDs exact. Positional search means a name substring. `search --fetch` explicitly fills a short cache result with one matching page, then rereads. It conflicts with favorites/pagination continuation. Preserve filters, page, mirror, pin and custom library in next-step guidance. New refresh rate limits remain one per two seconds and one active request.
- Add stable country/tag setup seeds. Validate the seed before setup effects. Repeating an identical resolved seed replays its request; the legacy unfiltered first-use ID stays unchanged.
- Add `sigy play STATION_UUID` and `sigy stop`. Explicit play preflights service/decoder/containment, reuses one exact linked audio revision or registers the first with redirects denied, prints a fresh listen identity and invokes the existing service-acquired pipe and bounded decoder. Ambiguous registrations and HLS still refuse. No directory click or hidden refresh occurs. Registration can survive a later failed listen.
- In the explorer, `P` freezes the selected UUID and asks for `y`; `n`/Escape discard it. `S` requests stop. Search/filter editing keeps these keys as text. The child gets argv-separated identity, bounded diagnostics, stdin and parent lifecycle fences. Service receipts drive the Live view; a hint or child existence does not prove playback. Quit leaves captures running and requests quick-client cleanup.
- Document a Waybar module and all six CSS states. Escape directory tooltip text for markup. Keep Linux/Omarchy/platform qualification false.

## Limits and follow-up

No new output adapter is introduced. Direct live system output still depends on FFmpeg device support; the bounded Windows helper remains retained-only. Null decoding is not audibility. Session hints remain advisory, and simultaneous client-start coordination is not upgraded into an atomic service-wide session lease. Retained segment navigation, TUI directory-fetch actions, city lookup, qualified devices and general planning remain separate packages. The Linux adapter needs real delegated topology and lifecycle evidence before support claims. No stage exits or supported release are established.

Verification must include normal lint/tests/audit, native media, per-crate 80% coverage, Unix capability failures before effects, retry/replay/cache preservation, panel admission beyond the first page, confirmation identity and production visual QA. Publish a source-only development checkpoint only after local gates and exact-commit CI pass.

Integrated review keeps large CLI futures on the heap and caps router frames at 16 KiB in a regression test after a retained-system playback stack overflow. The VM-work fixture uses a separate measurement-only wall ceiling; production query deadlines and all accounting/resource caps stay unchanged. Timing samples are observations, not portable latency guarantees.
