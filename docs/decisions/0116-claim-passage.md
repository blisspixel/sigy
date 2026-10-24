# 0116: Claim passage

Date: 2026-10-23

Status: implemented link from one claim sighting to one retained stored cue. No finding row is created. A cue that is not retained is refused. Expired and missing statements stay on findings. Catalog schema is v54. Local IPC is v57. Issue 14 stays open. A truth score, news fetch, and a stage exit are not claimed.

## Decision

A claim sighting can cite one stored finding, or one retained original-script cue together with one stored translation of that cue. The cue path does not insert a monitor finding.

The stored passage identity is the transcript id, transcript revision, translation revision, cue ordinal, recording id, original state `retained`, and that cue's own start and end. Monitor id and finding id are absent on that row. A finding sighting keeps its monitor and finding and leaves the cue columns empty. The same sighting id cannot be both.

The same passage citation changes nothing. A different citation for that id conflicts and writes nothing. Replay does not re-check the media and does not rewrite the row. A missing cue writes nothing. A cue whose recording is not retained writes nothing. The stored original state stays `retained` even if the recording later expires. `show` reports a newer original-script transcript, or a newer translation of the cited revision, as stale and does not rewrite the row.

Evidence and status still require the sighting. They still do not set a score. The basis for a passage has no finding id. Review reasons still name a later correction or a newer recognition or translation of that cue. The timeline says the page searched linked findings and stored cues, and that material outside those links was not examined.

`claim candidates` is unchanged. A hit with no finding is still not a candidate. The printed accept line stays the finding command.

Schema v54 rebuilds claim sightings, evidence, and status so a passage can be stored without a finding. Completion holds are not dropped or reinterpreted. Local IPC is v57 because `ClaimOperation` gains `Passage` and a passage sighting omits the finding fields. An existing finding payload keeps its shape.

This does not close issue 13 or issue 14. Phases 2 and 3 of issue 14 still need news items and a measured model. S-03B stays open. No stage exit is claimed.

## What this does not do

This slice does not fetch news, scrape a page, read a sitemap, or fetch `robots.txt`. It does not add a globe layer or an anomaly score. It does not turn a search hit into a passage. It does not store an expired or missing passage. It does not start playback from the claim page. It does not add a claim mutation to MCP. It does not change playback, settlement, or completion holds.

## Interface

`sigy claim sighting CLAIM SIGHTING --transcript ID --revision N --translation N --cue N` links one retained cue. `--monitor` and `--finding` still link one stored finding. Passing both, or passing only part of either set, writes nothing. `show` names the transcript, revision, translation, and cue for a passage. It does not print a monitor or finding for that row.

## Evidence

`cargo verify` exited 0 in 427.44 seconds. The `sigy` binary tests passed 406, with 7 ignored, in 23.58 seconds. The `sigy-service` library tests passed 773, with 2 ignored, in 178.44 seconds. `cargo audit --deny warnings` loaded 1352 advisories and scanned 382 dependencies. This slice does not change playback admission, recording publication, or HTTP acquisition, so `cargo verify-media` was not run. Coverage and `cargo visual-qa` were not run. External spend is USD 0. This receipt was written after those commands. Hosted verify of this commit is not recorded here.

## Limits

A later expiry does not flip a passage that was stored as retained. Candidates stay finding-only. Issue 14 stays open. Process exit before a completion hold commits can still drop that outcome. S-03B stays open. No acoustic, Omarchy, language, or platform qualification is claimed. External spend is USD 0. This slice does not change playback, so `cargo verify-media` is not required. Coverage and `cargo visual-qa` were not run.
