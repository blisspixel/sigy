# 0115: Latest sealed interval

Date: 2026-10-23

Status: implemented play of the newest playable sealed interval on the loaded recording map. The exclusive end is not a seek target. Prefetch, a free-form interior editor, device presentation, acoustic output, and a stage exit are not claimed. Catalog schema stays v53. Local IPC stays v56.

## Decision

Recordings can already play one chosen sealed interval and can already continue a whole-segment listen after that segment ends. This increment adds one confirmation that starts at the newest interval the loaded map can play. It does not add a second playback path.

`L` asks for confirmation. `y` or `Y` accepts. `n`, `N`, and Escape discard the request and start nothing. Capital `H` remains the heard pause. A confirmation consumes keys until it is accepted or discarded, so `H` does not pause and `,` or `.` do not move the cursor during that question. Lowercase `l` is unchanged.

The accepted request is one `listen file` at that interval's start. A start above zero passes `--seek-us`. No `--end-us` is passed, so the existing whole-segment handoff can continue when the reader completes. The open tail is not read. The loaded map is not refreshed by `L`. A seal that arrived after the last reload is not visible until `r`. The service still checks the catalog when the reader is admitted.

The newest playable interval is chosen from the end of the loaded map:

- A truncated map starts nothing.
- An empty map starts nothing.
- The open object is skipped when an earlier sealed interval can play. When it is the only candidate, nothing starts.
- A released interval, an empty length, and an overlapping gap, including a codec change, are skipped the same way. When the newest interval fails and no earlier interval can play, the newest refusal is shown and nothing starts.
- Storage other than `reserved` or `retained` starts nothing.

A sealed earlier interval can be the latest playable interval while capture is still running, when a later object is the open object. That listen does not stop capture.

A whole-segment listen whose admission is already on that recording at or after the chosen start is not restarted. Its follow wait, when one is active, stays. A cited listen stores an end, so it is replaced by this whole-segment listen. One foreground client still applies: a running child is stopped before the next listen starts. This request does not call `listen pause` or `listen seek`.

The question is `Play latest sealed interval [{start}, {end})? y confirms; n/Esc cancels. Capture keeps running.` A listen already on that interval notes `This listen is already on the latest sealed interval. Capture keeps running.` Refusals that the interval chooser already owns keep those sentences. No object key or path is placed on the status line. No catalog migration and no IPC message change. This does not exit DV-01C or DV-02. It does not close issue 13 or issue 14. It does not close S-03B.

## What this does not do

This slice does not seek to the exclusive end of the newest interval. It does not play the open tail or present that start as zero-latency reception. It does not stitch across a gap. It does not prefetch or admit a second reader while one child is decoding. It does not choose an arbitrary point inside an interval. It does not move the metadata playhead. It does not enable Play or Stop on the desktop desk. It does not change retention, settlement, completion holds, podcast refresh, or claim storage. It does not fetch news, scrape, read a sitemap, or fetch `robots.txt`. It does not add a score.

## Interface

Recordings help says `L` then `y` plays the latest sealed interval. The short hint includes `L latest`. `P` then `y`, `I` then `y`, `K` then `y`, and capital `H` stay in place.

## Evidence

`cargo verify` exited 0 in 357.98 seconds. The `sigy` binary tests passed 404, with 7 ignored, in 21.12 seconds. The `sigy-service` library tests passed 770, with 2 ignored, in 221.32 seconds. `cargo audit --deny warnings` loaded 1352 advisories and scanned 382 dependencies. `cargo verify-media` exited 0 in 136.47 seconds: 20 ignored service fixtures in 124.58 seconds and the direct live PCM check in 0.08 seconds. `SIGY_TEST_FFMPEG` was unset. FFmpeg on PATH is `N-113513-gaa3cfd4b5a-20240202` at `C:\Program Files\FFmpeg\bin\ffmpeg.exe`. Coverage and `cargo visual-qa` were not run. External spend is USD 0. This receipt was written after those commands. Hosted verify of this commit is not recorded here.

## Limits

The exclusive end of the newest interval is not a seek target. A stale loaded map can miss a seal that arrived after the last reload. Prefetch and a free-form interior editor stay later. Process exit before a completion hold commits can still drop that outcome. S-03B stays open. Issues 13 and 14 stay open. No acoustic, Omarchy, language, or platform qualification is claimed. External spend is USD 0. This slice starts a retained `listen file`, so `cargo verify` and `cargo verify-media` are both required. No new FFmpeg fixture is added.
