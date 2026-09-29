# Chunked recognition

Date: 2026-09-29. Status: implemented and tested on Windows x86_64. Catalog schema and local IPC are v34. This is increment 4 of the [scaling architecture](../design/scaling-architecture.md#increments). It amends the one-interval ceiling in the [recognition storage API](0036-recognition-storage-api.md), the single-interval job in the [native recognition worker](0039-native-recognition-worker.md), and the long-recording skip in [monitor processing](0048-monitor-processing.md). No language is qualified.

## Decision

One `local_asr` job still publishes one transcript revision. Admission splits the retained timeline into chunks of at most 30 seconds. That grid fixes the input manifest. A remainder, including a 1 microsecond tail, is its own chunk. Abutting segments stay separate. A gap or an uncovered hole produces no chunk. At most 1,024 chunks are admitted. The byte sum stays at most 64 MiB (67,108,864 bytes). Cue count stays at most 256 and text stays at most 65,536 bytes. Crossing either cap fails the whole job as an invalid worker result and inserts nothing. Translation cue and ordinal caps stay as they are. Published coverages can end earlier than this grid.

The input manifest is `sigy-local-asr-input-v2`. A queued job that still carries the previous manifest fails claim as `input-no-longer-current`. Its stored manifest is left unchanged, and the job is not replanned at finish. A whole-segment first chunk omits the file offset, the chunk ordinal and the segment end from the task spec, so the pinned golden spec hash is unchanged. Any other window records a slice. A window that ends before its segment also records the segment end. The combined success digest is the SHA-256 of `sigy-local-asr-chunks-v1` and the ordered window spec hashes.

Decode stays on `pipe:0` with the pipe protocol whitelist. A whole segment uses the historical FFmpeg argument list. Any other chunk adds `-ss` and `-t` after `-i pipe:0` and before `-map`. FFmpeg receives no URL. The decoder deadline stays 60 seconds, committed memory stays 512 MiB, and the decoder is still one process. PCM is not padded to 30 seconds.

Each recognizer process hears at most 30 seconds of one segment. When a phrase reaches that window's edge and the segment continues, the phrase waits. The next window starts at the phrase, so the phrase is heard from its start and stored once. Silence advances the full window. A phrase that already starts at the window cursor and runs to the edge is stored through the window end, because the recognizer gave no earlier break; a word inside that phrase can still be cut. The break is the recognizer's own segment timestamp, on about a 20 millisecond grid. No text from the previous window is passed in as a prompt.

Published coverages tile each segment. They abut, they do not overlap, and each new row is at most 30 seconds and at most `sample_rate * 30` samples. The sample count agrees with the duration within one sample. The stored hash and sample count describe that published prefix. The process also heard through the end of its window, which can be longer than the prefix. The number of published coverages can differ from the admission chunk count. A migrated catalog may still hold one coverage of up to 60 seconds: the column check stays at 60 seconds so that row remains valid, and the insert trigger caps new rows at 30 seconds. Coverage ordinals are `0..n-1`. A cue lies inside exactly one coverage. A cue that crosses a coverage or a gap publishes nothing.

The recorded lease is the profile deadline plus 600,000 ms. It is not multiplied by the window count. The recognition slot stays one, so one recording holds it until every window of that job finishes. Restart redoes every window. There is no stored window cursor. Cancellation between windows finishes cancelled after that process group drains, and the read lease stays until the drain. A hard failure in any window fails the whole job and publishes nothing.

Language evidence stays one row: the id equals the job id, revision 1, origin `recognizer`, resolution `block`, capability `unevaluated`, alias map `whisper-cpp-codes-v1`. A published coverage that produced text contributes one span bounded to that coverage. Span ordinals count emitted spans. A window whose phrase was deferred, and a silent window, add coverage and no span. An all-silent result is `no_text` and stores no language row. Each window still runs with `language=auto`. No window receives the previous window's label. If language-evidence validation fails, the transcript still stands.

Monitor caps are charged once for the recording's decoded duration. Migration deletes recognition skips whose reason is `recognition-input-limit`, so those monitors can retry. Other skip reasons stay. A manual `analysis transcribe` does not read that row.

`analysis transcript` prints each published coverage with its place in the interval, its media clock and its sample count.

A transcript page still attaches every coverage row. 208 worst-case coverage rows plus a full transcript summary fit in one 65,536-byte page. 209 rows do not. A 15-minute capture is about 30 windows and fits. A plan whose worst-case page would exceed 65,536 bytes is refused before admission. If the next published coverage would not fit on that page, the job fails closed before the window and publishes nothing.

Rebuilding `transcript_coverage` drops the triggers that reference it before `DROP TABLE`. A test downgrade that compares `sqlite_schema` with a file-built older catalog reverses this migration's widened file-count check and restored the v30 recognition admission trigger. A catalog that already contains the widened check skips that rebuild and still applies the rest of v34.

## Evidence

Planner tests cover a 60,000,000 us span as two chunks, a 60,000,001 us span as three chunks, a 1 us tail kept as its own chunk, a maximum of 1,024 chunks, and holes that emit nothing. Storage tests publish a 90-second file as three chunks with cues in the first and third, language spans only for nonempty chunks, and a zero-USD decision. A cue from 29 seconds to 31 seconds publishes nothing. Two sealed segments stay two chunks. A 60-second file whose admission plan is two chunks publishes three abutting coverages, from 0 to 22 seconds, 22 to 52 seconds, and 52 to 60 seconds, with cues inside those rows. A cue that crosses one of those cuts publishes nothing. A gap between coverages publishes nothing. A phrase that reaches a window edge while the segment continues is omitted, and the stored hash is the prefix before that phrase. A phrase that fills the window from its start is stored through the window end. Silence advances a full window and stores no language span. A 60-second v33 coverage migrates with ordinal 0 and stays readable. A queued pre-v34 manifest fails claim as `input-no-longer-current`. An interrupted migration stays at v33. A v30 rewind matches the file-built schema and preserves rows and leases. Backup and restore of one recognized chunk reopens. The golden whole-segment spec hash is unchanged. A window that ends before its segment records the segment end; a window that finishes the segment omits it. The 3-second whole-segment decoder argument list is unchanged. A sliced window inserts `-ss` and `-t` after `pipe:0`.

On 2026-09-29, `cargo verify` passed 469 tests, with 15 native-media tests ignored, warnings-denied Clippy, a locked build, and `cargo audit` of 312 crate dependencies against 1,277 advisories. `cargo verify-media` passed 15 of 15 on FFmpeg 9.0.1. The suspended-spawn window did not recur in this run. This is one Windows host, not a platform matrix.

## Limitations

- The recognition slot is one. One recording holds it until all of its windows finish. Fair scheduling is the next increment.
- A phrase that fills a window from its start, with no earlier timestamp, is stored through the window end. A word at that edge can still be cut. These timestamps are the recognizer's phrase segments, not a word-level alignment.
- Output seeking can miss the 60-second decoder deadline on a late slice of a long file.
- The recorded lease is one profile deadline plus 600,000 ms, however many windows the job has.
- Cue count stays at most 256 and text stays at most 65,536 bytes. The byte cap stays 64 MiB.
- 208 worst-case coverage rows fit one read page. A longer plan is refused before admission. A job whose next coverage would exceed that page fails closed and publishes nothing.
- Each window runs with `language=auto`. The previous window's label is not passed forward.
- Linux delegated-cgroup containment is unchanged. The suspended-spawn assignment window remains. A Job Object is not a network sandbox.
- No language is qualified, and no platform is qualified.
- The one-chunk backup fixture does not finish operation 38.
