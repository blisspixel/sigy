# 0084: Bounded Windows retained audio output

Date: 2026-10-04. Status: implemented increment; measured outcomes and verification belong in [active work](../development/progress.md). This extends DV-01A without completing the DVR or S-04 native qualification.

## Decision

Windows retained `listen file` and `listen play --destination system` decode the protected service byte stream to interleaved little-endian floating-point PCM and send it to a supervised local output helper. The service still owns and protects the original recording. The configured decoder remains user-selected. `--destination null`, direct live listening and other platforms keep their existing paths. There is no source contact, microphone capture, volume change, model request or automatic fallback in this output adapter.

Pin CPAL 0.18.2 with default features disabled, as a Windows-only application dependency. A fixed internal same-binary helper entry point bypasses library and CLI initialization and receives only finite duration and PCM frames on its standard input. It receives no recording path, source URL, credentials, library operation or executable selection. It writes bounded structured handshake and completion messages. The entry point is absent from the public command parser and MCP.

CPAL device discovery, stream creation, start and destruction all occur inside the child. Its native Windows stream destruction can synchronously join a worker; an asynchronous in-process timeout cannot bound that join. Alternatives and the complete retained dependency graph are recorded in [the audio research](../../research/44-bounded-windows-audio-output.md).

## Output profile and resource bounds

Freeze the current default output endpoint's identity before opening it, and refuse an unavailable or changed endpoint rather than silently changing routes. This first profile accepts floating-point mono or stereo at 8 to 192 kHz. Other default formats and layouts refuse explicitly. Request a 10 ms buffer and validate the actual estimate; callback sizes above 250 ms fail. These limits select one profile, not platform-wide support.

The single-producer/single-consumer ring holds at most 250 ms of whole frames, below 1 MiB. The callback allocates nothing, logs nothing, takes no locks and opens no files. Pre-roll is at most 50 ms, shortened for shorter media. The parent PCM channel has exactly two 8 KiB chunks and a fixed 8,200-byte frame-alignment buffer. Each child frame is capped at 32 KiB; configuration, message and total PCM bounds are independently checked. Maximum PCM is the ceiling of requested duration plus 100 ms, multiplied by the negotiated sample rate, channels and four bytes per sample. A zero-length end frame requires nonempty complete input and immediate EOF.

Preserve every finite decoded sample in transport. Saturate finite values outside [-1,1] only when publishing a frame to the device ring, and count clipped scalar samples once. Full-ring retries count nothing. Signed zero is preserved; NaN and infinity refuse before frame publication. Stored originals are unchanged. Clipping is an observable output concession, not transcript or media correction.

The existing native containment mechanism bounds the output helper and decoder together to two processes, 768 MiB committed memory and 0.75 CPU cores. Startup negotiation is bounded to five seconds; overall work uses the complete encoded file duration plus 30 seconds so seek-prefix decoding has time. Helper end processing checks the existing cooperative deadline; estimated final presentation has a separate three-second drain bound. Cancellation or failure requests group termination and then the existing five-second actual empty-group observation. The group seals before draining so no later spawn can invalidate that observation. These are per-operation limits, not aggregate host admission or measured capacity.

The helper's cooperative feeding horizon starts after negotiation and permits the requested remaining media duration plus five seconds, including checksum waits, prefix decoding and backpressure. It can refuse before the parent's outer horizon, especially on large cold files or a short seek near a long file's end. The 30-minute and 512 MiB admission ceilings establish neither cold-start latency nor playback capacity. Cold-read, late-prefix and sustained-listening qualification remains open.

## Separate evidence and completion

Decoder completion means successful native decoding with independently bounded progress and exact forwarded PCM byte accounting. Output completion requires one validated final report, clean protocol EOF, successful helper exit and observed empty native group. Reported content frames must equal decoded frames; clipped samples, underruns, drain zeros, callbacks, queue high water and final presentation time have explicit arithmetic bounds.

Initial FFmpeg `N/A` timestamp observations stay unknown and consume the existing work bounds. They do not become zero-time measurements or establish progress. The final frame must have a measured timestamp; unknown observations after a measured value, conflicts, signed values, regressions and excessive times refuse. Failed helper reports preserve only fixed validated failure codes, never a completion claim.

Callback submission and predicted presentation do not prove acoustic delivery. Successful reports state that presentation is estimated and audibility is unproven. Decoder failure, output failure, native closure and original-reader outcome remain separate, including a completed decoder followed by a failed device. The service releases original-file protection only from its own actual joined-reader evidence; output reports cannot release it. Exact retained request replay starts no reader, decoder or helper.

Native group Drop requests termination and does not certify closure. The current raw spawn suspends a child before group assignment, leaving an abrupt-parent-death assignment window. Some extra process creation flags are not retained by that mechanism. Device disconnection, physical sound delivery, abrupt parent death, hostile-code network isolation, decoder identity qualification, long listening sessions and contended multi-client capacity remain open evidence gates. A real sleeping-child termination test is not a WASAPI driver-hang qualification. No schema or IPC change is required by this output increment.

## Verification

Use independent hand-declared stereo frames, malformed framing and strict EOF controls, counter overflow, output-only clipping and full-ring retry checks. Exercise an actual silent child that stalls, then require termination, empty-group accounting and rejection of later spawn. A retained native fixture compares every decoded PCM sample against an independently generated one-second stereo source with seek sentinels, and checks original-reader completion separately.

Actual device execution requires an explicit private opt-in with a new private receipt and a known one-second low-amplitude source. An unavailable device fails that requested qualification; it does not become a passing skipped outcome. Save the raw process result before validation. Full ordinary, native-media and per-crate coverage gates remain required. One endpoint run cannot qualify the platform, acoustic delivery or a complete DVR.
