# 0112: Latest-sealed follow

Date: 2026-10-23

Status: implemented wait for the next sealed segment after a whole-segment listen. Prefetch, a range editor, device presentation, acoustic output, and a stage exit are not claimed. Catalog schema stays v53. Local IPC stays v56.

## Decision

[Decision 0107](0107-heard-segment-handoff.md) already chooses `Jump` when one whole-segment listen has exited, its reader completed, a decoder sample exists, and the exclusive end is past the origin. A cited listen does not continue. A running child does not prefetch. This increment does not change those conditions.

`Jump` no longer starts `listen file` in the same poll. It arms one follow slot. The slot stores the recording id and that exclusive end. On the explorer's existing one-second clock, and only when no show is in flight, it reads `RecordingOperation::Show` for that id. The first observation is therefore up to about one second after the child exits. A sealed next interval now starts one `listen file` after that show, with `--seek-us` and no `--end-us`. An open tail never starts a decoder and never opens a source URL.

The show is the authority. The glyph timeline is not. The map keeps the recording state, the open ceiling, the open object key, and at most 1,024 intervals and 1,024 gaps, the same cap as the timeline projection. A longer map is truncated and stops. It does not start audio and it does not wait, because a partial map could name the wrong file. The state copy keeps the first 32 characters, which preserves `running`.

The open tail matches the service: state `running`, open ceiling above zero, and an open object key. `locate` then decides:

- A segment whose object key is not the open object starts one listen, including when capture has already stopped. Two sealed segments on a finished recording still hand off.
- A segment whose object key is the open object waits only while that tail is open. Otherwise it stops.
- An open tail waits only while that tail is open. Otherwise it stops.
- A gap, including a codec-change gap, an outside edge, an unpublished recording, and a released edge stop. Nothing is started.

A codec change is already a seal-time gap. `seal_segment` refuses a different format, journals that gap, and fails the capture. This slice does not detect a codec change in the player and does not start an adjacent file of another codec.

The wait is one show per clock tick, one request in flight, for at most `RETAINED_READ_DEADLINE_SECONDS` (30 minutes). The waiting sentence is printed once, and only after the first show says wait. A show error does not start audio. The same static fault is noted once, with no path, and the next clock retries until the deadline. At the deadline the slot prints the give-up sentence, drops any in-flight show, and does not start from a late result. `H`, `[`, `]`, `S`, quit, and a new confirmed play clear the slot first. The child has already exited, so those keys do not call `listen pause` or `listen seek`. After cancel, the completed request stays explained, so the wait is not armed again. If the later `listen file` writes no reader, the position stays and the wait is not armed again.

The sentences are:

- `Waiting for the next sealed segment. The open tail was not started. Capture keeps running.`
- `The wait for the next sealed segment ended. The open tail was not started. Capture keeps running.`
- `No sealed segment contains that end. Nothing was started. Capture keeps running.`
- `The recording map could not be read. The open tail was not started. Capture keeps running.`

No catalog migration and no IPC message change. This does not exit DV-01C or DV-02. It does not close issue 13 or issue 14. It does not close S-03B.

## What this does not do

This slice does not prefetch the open tail, admit a second reader while one is protected, or open a source URL. It does not add a range editor. It does not move the metadata playhead. It does not enable Play or Stop on the desktop desk. It does not change settlement, completion holds, podcast refresh, or claim storage. It does not fetch news, scrape, read a sitemap, or fetch `robots.txt`. It does not add a score.

## Interface

Recordings help says the next sealed segment starts once it is sealed and an open tail waits. `P` then `y` and Findings playback are unchanged. A cited listen still does not continue.

## Evidence

`cargo verify` exited 0 in 403.4 seconds. The `sigy` binary tests passed 390, with 7 ignored, in 26.30 seconds. The `sigy-service` library tests passed 770, with 2 ignored, in 185.54 seconds. `cargo audit --deny warnings` loaded 1351 advisories and scanned 382 dependencies. `cargo verify-media` exited 0 in 199.32 seconds: 20 ignored service fixtures in 113.13 seconds and the direct live PCM check in 0.08 seconds. FFmpeg is `N-113513-gaa3cfd4b5a-20240202`. Coverage and `cargo visual-qa` were not run. External spend is USD 0. This receipt was written after those commands. Hosted verify of this commit is not recorded here.

## Limits

Prefetch and the range editor stay later. Process exit before a completion hold commits can still drop that outcome. S-03B stays open. Issues 13 and 14 stay open. No acoustic, Omarchy, language, or platform qualification is claimed. External spend is USD 0. This slice starts a retained `listen file`, so `cargo verify` and `cargo verify-media` are both required. No new FFmpeg fixture is added.
