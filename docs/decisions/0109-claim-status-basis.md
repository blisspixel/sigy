# 0109: Claim status basis

Date: 2026-10-23

Status: implemented read-time basis for one stored claim status. Catalog schema stays v53. Local IPC is v55. Issue 14 stays open. A truth score, news fetch, and a stage exit are not claimed.

## Decision

[Decision 0104](0104-manual-claims.md) stores a status the user sets and cites an evidence link. A later transcript or translation is already labeled stale on the sighting. The status row itself stayed silent, so a corroborated decision could still read as current after the cited words or the original changed.

This increment adds that check to the claim page. It runs when the page is read. It inserts no status row, no sighting, and no evidence. The stored token, revision, setter, and evidence id stay as the user left them. A later `claim status` command is still the only way to record a new decision. That command appends. It does not erase the earlier row.

The basis is `none` when no status row exists, including when a sighting is already stale. It is `current` when a status exists and none of the reasons below apply. It is `review` when one or more apply. The reasons, in this order, are:

- `correction`: a newer transcript revision of the cited finding has kind `correction` and outcome `text`.
- `superseded`: a newer transcript revision has kind `recognition` and outcome `text`, or a newer translation exists for the cited transcript revision. Withdrawal and supersession share this one token.
- `missing-media`: the cited finding's stored original state is `missing`.
- `retention-expired`: that stored original state is `expired`.

The check follows the latest status to its evidence, that evidence to its sighting, and that sighting to its finding. Another claim is reviewed only when its own chain matches. A correction of a transcript reviews every decision whose cited finding uses that transcript. A missing or expired original on one finding does not review a decision that cites a different finding. Missing media and retention expiry come from the finding's stored original state, the same states [stored findings](0057-stored-findings.md) already record. They can combine with a text reason. The page does not reclassify a finding that was stored as retained after the bytes later leave. An unknown original state, or a status whose evidence chain is no longer in the catalog, is a catalog-integrity fault.

Frozen briefing generations and redacted exports are not opened or rewritten. The topic-monitoring rule still holds: a stored finding keeps its cited revision and reports staleness on read. This page explains the later change beside the historical status. It does not mark the claim disputed, retracted, or corroborated.

## What this does not do

This slice does not search the library, accept a passage that is not a stored finding, or insert a claim from analysis search. It does not fetch, scrape, or store a news item. It does not add a score, an anomaly table, or an MCP tool that creates or changes a claim. `claim_list` and `claim_show` only read. It does not close issue 14. Later phases still need news items and a measured model. It does not close issue 13. It does not close S-03B. It does not exit a roadmap stage.

## Interface

`sigy claim show` prints the stored status, then `Basis: none`, `Basis: current`, or `Basis: review` with the reason tokens and the evidence, sighting, and finding ids. The timeline still precedes the sightings. `--json` is the same page. The basis object carries `state`, the three ids when a status exists, and `reasons`. Unverified omits the ids. There is still no score field. Local IPC is v55 because the claim page is a required field on a message that rejects unknown fields. Catalog schema stays v53. No claim table changes.

## Evidence

`cargo verify` exited 0 in 599.20 seconds. The `sigy` binary tests passed 382, with 7 ignored, in 26.47 seconds. The `sigy-service` library tests passed 760, with 2 ignored, in 183.94 seconds. `cargo audit --deny warnings` loaded 1351 advisories and scanned 382 dependencies. Coverage and `cargo visual-qa` were not run. `cargo verify-media` was not required. External spend is USD 0. This receipt was written after those commands.

## Limits

Issue 14 stays open. A passage that is not a stored finding is still refused. The stored relation labels are still the user's words, not behavioral fixtures. Phases that need news items or a measured model are not this slice. Issue 13 stays open. S-03B stays open. Process exit before a completion hold commits can still drop that outcome. No acoustic, Omarchy, language, or platform qualification is claimed. External spend is USD 0. This slice does not change recording, playback, or HTTP acquisition, so `cargo verify-media` is not required. The same gate moves Tokio from 1.53.1 to 1.53.3. RUSTSEC-2026-0386 covers the Unix `AsyncFd` constructors. Sigy does not call them. Separate research experiment locks remain on 1.53.1.
