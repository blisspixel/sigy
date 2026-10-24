# 0113: Confirmed sealed interval

Date: 2026-10-23

Status: implemented play of one sealed interval already on the loaded recording map. Prefetch, a free-form interior editor, device presentation, acoustic output, and a stage exit are not claimed. Catalog schema stays v53. Local IPC stays v56.

## Decision

Recordings already holds a seal map for the selected row: at most 1,024 intervals and 1,024 gaps, the recording state, the open ceiling, and the open object key. This increment plays one of those intervals. It does not refresh the map, and it does not start a decoder while choosing.

`,` and `.` move a cursor across that loaded list and stop at the ends. The move writes no reader and starts no audio. A partial map, or a map with no intervals, only notes the refusal. `I` asks for confirmation. `y` or `Y` accepts. `n`, `N`, and Escape discard the request and contact nothing.

The accepted listen is one `listen file` for that interval's own half-open bounds. A start above zero passes `--seek-us`. The exclusive end always passes `--end-us`. The child is the existing foreground client, with `--destination system`, `--cancel-on-stdin`, and `--parent-pid`. If a child is still running, the existing queue stops it and starts this listen only after that child has exited and its diagnostic drain has finished. One decoder. This slice does not admit a second reader while the first child is decoding.

The terminal refuses before spawn when the loaded map cannot name a playable file:

- A truncated map could name the wrong file, so it starts nothing.
- A missing index starts nothing.
- An interval whose object key is the open object starts nothing, including when capture has stopped and the key is still set. The service excludes that object.
- A released interval starts nothing.
- An interval whose start is not before its end starts nothing.
- An interval that overlaps a gap, including a codec-change gap, starts nothing. A gap that begins at the exclusive end does not overlap.
- A recording whose storage is neither `reserved` nor `retained` starts nothing.

A sealed earlier interval can play while capture is still running when its object is not the open object and storage is `reserved` or `retained`. `P` then `y` is unchanged: one whole retained recording that is not capturing, with no `--seek-us` and no `--end-us`.

The service still checks the catalog at admission. An end past that sealed interval, or a range that overlaps a gap, is refused there and writes no reader. The loaded map is not a second authority.

The new listen stores the exclusive end. Handoff treats that end as a cited listen and does not continue. A whole-segment handoff still omits `--end-us`. A later ten-second seek inside this listen keeps the same end. That step is unchanged and is not a typed editor. A cited finding stays `listen finding`.

The sentences are:

- `This recording map is partial. No interval was started. Capture keeps running.`
- `No sealed interval is selected. Nothing was started. Capture keeps running.`
- `That interval is the open tail. Nothing was started. Capture keeps running.`
- `That interval is released. Nothing was started. Capture keeps running.`
- `That interval overlaps a gap. Nothing was started. Capture keeps running.`
- `That interval has no positive length. Nothing was started. Capture keeps running.`
- `This recording is not reserved or retained. Nothing was started. Capture keeps running.`
- `No selected recording. No interval was started. Capture keeps running.`
- `No selected recording. No interval was chosen. Capture keeps running.`
- `Play sealed interval [{start}, {end})? y confirms; n/Esc cancels. Capture keeps running.`

No object key or path is placed on the status line. No catalog migration and no IPC message change. This does not exit DV-01C or DV-02. It does not close issue 13 or issue 14. It does not close S-03B.

## What this does not do

This slice does not choose an arbitrary point inside an interval. It does not stitch across a gap or a codec change. It does not prefetch, admit a second reader while one child is decoding, or open a source URL. It does not move the metadata playhead. It does not enable Play or Stop on the desktop desk. It does not change settlement, completion holds, podcast refresh, or claim storage. It does not fetch news, scrape, read a sitemap, or fetch `robots.txt`. It does not add a score.

## Interface

Recordings help says `,` and `.` choose a sealed interval and `I` then `y` plays it. The tall recording body names the interval under the cursor. The compact body does not add that line. `P` then `y` and Findings playback are unchanged. The follow help line stays.

## Evidence

`cargo verify` exited 0 in 331.66 seconds. The `sigy` binary tests passed 395, with 7 ignored, in 23.88 seconds. The `sigy-service` library tests passed 770, with 2 ignored, in 186.43 seconds. `cargo audit --deny warnings` loaded 1351 advisories and scanned 382 dependencies. `cargo verify-media` exited 0 in 117.6 seconds: 20 ignored service fixtures in 109.63 seconds and the direct live PCM check in 0.07 seconds. FFmpeg is `N-113513-gaa3cfd4b5a-20240202`. Coverage and `cargo visual-qa` were not run. External spend is USD 0. This receipt was written after those commands. Hosted verify of this commit is not recorded here.

## Limits

Prefetch and a free-form interior editor stay later. Process exit before a completion hold commits can still drop that outcome. S-03B stays open. Issues 13 and 14 stay open. No acoustic, Omarchy, language, or platform qualification is claimed. External spend is USD 0. This slice starts a retained `listen file`, so `cargo verify` and `cargo verify-media` are both required. No new FFmpeg fixture is added.
