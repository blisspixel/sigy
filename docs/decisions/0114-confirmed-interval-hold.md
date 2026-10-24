# 0114: Confirmed interval hold

Date: 2026-10-23

Status: implemented hold of one sealed interval already on the loaded recording map. Prefetch, a free-form interior editor, a return to the latest playable position, device presentation, acoustic output, and a stage exit are not claimed. Catalog schema stays v53. Local IPC stays v56.

## Decision

`record hold` already protects every published segment a half-open range intersects and records each gap inside that range. The open tail is not a segment and is not protected. Each call inserts a new hold. This increment exposes that command for one interval the Recordings cursor already names. It does not add a second retention path.

`,` and `.` still move the cursor and write nothing. `K` asks for confirmation. `y` or `Y` accepts. `n`, `N`, and Escape discard the request and record nothing. Capital `H` remains the heard pause. A confirmation consumes keys until it is accepted or discarded, so `H` does not pause during that question.

The accepted request is one `RecordingOperation::Hold` for that interval's own half-open bounds. The terminal reuses the sealed-interval refusals, so it offers a hold only for an interval it could play:

- A truncated map records nothing.
- A missing index records nothing.
- The open object records nothing.
- A released interval records nothing.
- An empty length records nothing.
- An overlapping gap, including a codec change, records nothing. The CLI command remains the wider path and can record a gap inside an arbitrary range.
- Storage other than `reserved` or `retained` records nothing.
- A disconnected client records nothing.

A sealed earlier interval can be held while capture is still running when its object is not the open object. The hold does not stop capture and does not start a decoder. `P` then `y` and `I` then `y` are unchanged.

One hold may be in flight. A second `K` while that request is unsettled notes the wait and does not insert another row. The reply must be one recording page for that id and must list the confirmed bounds. The newest matching ordinal is shown. The row keeps at most eight hold ranges and the full count. A stale generation is ignored. A metadata reload that started before the commit can still draw the previous row until the next reload. The catalog hold is not undone by that drawing. A disconnect clears the in-flight fence. A reply that arrives after that note does not change the row. If the hold had already committed, the next reload shows it.

The sentences are:

- `Hold sealed interval [{start}, {end})? y confirms; n/Esc cancels. Capture keeps running.`
- `Hold request discarded. No retention change was recorded. Capture keeps running.`
- `Recording one hold on that sealed interval. Capture keeps running.`
- `A hold is still settling. It was not repeated. Capture keeps running.`
- `Hold request ended without a result. It was not repeated. Capture keeps running.`
- `Hold {ordinal} recorded on [{start}, {end}). Capture keeps running. Another confirmation records another hold.`
- `Disconnected. The hold was not submitted. r reconnects. Capture keeps running.`
- `No selected recording. No hold was recorded. Capture keeps running.`

Refusals that the interval chooser already owns keep those sentences. No object key or path is placed on the status line. No catalog migration and no IPC message change. This does not exit DV-01C or DV-02. It does not close issue 13 or issue 14. It does not close S-03B.

## What this does not do

This slice does not choose an arbitrary point inside an interval. It does not hold a range that overlaps a gap. It does not change Keep, Archive, Temporary, or delete. It does not make a hold idempotent: a later confirmation inserts another row. It does not start, pause, seek, or stop playback. It does not move the metadata playhead or return to the latest playable position. It does not prefetch. It does not enable Play or Stop on the desktop desk. It does not change settlement, completion holds, podcast refresh, or claim storage. It does not fetch news, scrape, read a sitemap, or fetch `robots.txt`. It does not add a score.

## Interface

Recordings help says `K` then `y` records one hold on that interval. The short hint includes `K hold`. The tall recording body names the newest shown hold when one exists. The compact body does not add that line. `H` pause, `I` then `y`, and `P` then `y` stay in place.

## Evidence

`cargo verify` exited 0 in 338.21 seconds. The `sigy` binary tests passed 399, with 7 ignored, in 25.36 seconds. The `sigy-service` library tests passed 770, with 2 ignored, in 185.02 seconds. `cargo audit --deny warnings` loaded 1352 advisories and scanned 382 dependencies. This slice does not change playback admission, recording publication, or HTTP acquisition, so `cargo verify-media` was not run. Coverage and `cargo visual-qa` were not run. External spend is USD 0. This receipt was written after those commands. Hosted verify of this commit is not recorded here.

## Limits

Prefetch, a free-form interior editor, and a return to the latest playable position stay later. A reload that reads the recording before this hold commits can hide the new row until the next reload. Process exit before a completion hold commits can still drop that outcome. S-03B stays open. Issues 13 and 14 stay open. No acoustic, Omarchy, language, or platform qualification is claimed. External spend is USD 0.
