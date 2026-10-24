# 0117: Retained cue candidates

Date: 2026-10-23

Status: implemented read of one retained cue on `claim candidates` when no stored finding cites that hit. The search inserts nothing. A cue that is not retained is omitted. Catalog schema stays v54. Local IPC is v58. Issue 14 stays open. A truth score, news fetch, and a stage exit are not claimed.

## Decision

`claim candidates` keeps a search hit that a stored finding already cites. It also keeps a hit that no finding cites when that cue is retained. The retained check is the same one a passage sighting uses. A missing cue, a cue that is not original text, or a cue whose recording is not retained is omitted. A hit with no translation revision stays omitted.

A finding hit and a passage hit are one row. When findings are present, the row is a finding candidate. When no finding matches and the cue is retained, the findings list is empty. The same hit is not listed twice. A finding that cites an older translation is not attached to the newest translation. That newest retained cue is a passage candidate.

The command writes nothing. It does not create a finding, a claim, or a sighting. The printed accept line for a passage is the existing sighting command with `--transcript`, `--revision`, `--translation`, and `--cue`. The user replaces the claim and sighting ids. A finding accept line stays `--monitor` and `--finding`.

Schema stays v54. No claim table changes. Local IPC is v58 because a candidate page can now include a retained cue whose findings list is empty. An older client would show that row without an accept line. Completion holds are unchanged.

This does not close issue 13 or issue 14. Phases 2 and 3 of issue 14 still need news items and a measured model. Expired and missing cues still need a finding. S-03B stays open. No stage exit is claimed.

## What this does not do

This slice does not fetch news, scrape a page, read a sitemap, or fetch `robots.txt`. It does not add a globe layer or an anomaly score. It does not insert a sighting from the candidate list. It does not store an expired or missing passage. It does not start playback. It does not add a claim mutation, or `claim candidates`, to MCP. The explorer does not run this search. It does not change playback, settlement, or completion holds.

## Interface

`sigy claim candidates` takes the same arguments as `analysis search`. It prints how many hits were read and how many are candidates. A passage row names the transcript, revision, translation, and cue, then one accept line. A finding row names each finding and the finding accept line. An empty page says no stored finding or retained cue matched, and that a cue that is not retained is not a candidate. `--json` is the same page and has no score field.

## Evidence

`cargo verify` exited 0 in 441.44 seconds. The `sigy` binary tests passed 407, with 7 ignored, in 26.76 seconds. The `sigy-service` library tests passed 774, with 2 ignored, in 185.29 seconds. `cargo audit --deny warnings` loaded 1352 advisories and scanned 382 dependencies. This slice does not change playback admission, recording publication, or HTTP acquisition, so `cargo verify-media` was not run. Coverage and `cargo visual-qa` were not run. External spend is USD 0. This receipt was written after those commands. Hosted verify of this commit is not recorded here.

## Limits

A later expiry removes that cue from a later candidate page. A passage that was already stored as `retained` stays stored that way. Issue 14 stays open. Process exit before a completion hold commits can still drop that outcome. S-03B stays open. No acoustic, Omarchy, language, or platform qualification is claimed. External spend is USD 0. This slice does not change playback, so `cargo verify-media` is not required. Coverage and `cargo visual-qa` were not run.
