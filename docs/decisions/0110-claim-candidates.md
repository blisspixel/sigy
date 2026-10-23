# 0110: Claim candidates

Date: 2026-10-23

Status: implemented read of analysis-search hits that a stored finding already cites, plus a read-only claim page in Findings. Catalog schema stays v53. Local IPC is v56. Issue 14 stays open. A truth score, news fetch, and a stage exit are not claimed.

## Decision

Analysis search already reads stored cues. A user who wants to link one of those cues to a claim still has to know the monitor and finding ids. This increment lists the hits that a stored finding already cites, and it prints the existing `sigy claim sighting` command. It inserts no claim, sighting, evidence, or status.

The join is exact: transcript id, transcript revision, translation revision, cue ordinal, and recording id. A hit with no translation revision is omitted. A hit whose translation revision is not the one the finding cites is omitted. A current search pairs the cue with the newest translation, so a finding that cites an older translation is not a candidate for that hit. `--history` follows analysis search: an older translation is added only when the term is absent from the original and present in that older English text. That hit is labeled translation stale. Searching the original wording again, with or without `--history`, stays on the newest translation. A passage that is not a stored finding stays refused by `claim sighting`.

Each hit names at most eight findings, ordered by monitor id then finding id. A ninth match sets `more_findings` and keeps the first eight. An original state outside `retained`, `missing`, and `expired` is a catalog integrity failure. The search page bounds stay the analysis-search bounds. This command writes nothing.

The explorer does not gain an eighth workspace. The tab bar at 80 columns already fills with workspaces 1 through 7. Findings `c` opens the stored claim ids. Enter reads one claim. Escape returns to the list, and Escape again returns to findings. `r` reloads the open list or claim. Any other key on that page, including `P` and `S`, states that the page is read-only and starts no playback. `H`, `[`, and `]` still control an already running retained listen, because those keys are handled before the claim page. Leaving Findings closes the page. `claim candidates` stays a CLI read. The explorer does not run it. `claim_list` and `claim_show` remain the only claim MCP tools. Both only read.

Local IPC is v56 because `ClaimOperation` and `ClaimPage` gain a `Candidates` variant, and those messages reject unknown fields. Catalog schema stays v53. No claim table changes.

## What this does not do

This slice does not accept a passage that is not a stored finding. It does not insert a sighting from the candidate list. It does not fetch, scrape, or store a news item. It does not add a score, an anomaly table, or an MCP tool that creates or changes a claim. It does not close issue 14. Later phases still need news items and a measured model. It does not close issue 13. It does not close S-03B. It does not exit a roadmap stage. It does not start playback from the claim page.

## Interface

`sigy claim candidates` takes the same arguments as `analysis search`. It prints how many hits were read and how many are candidates, then each cue, its stale flags, and each matching finding. The accept line is `Accept with: sigy claim sighting CLAIM SIGHTING --monitor MONITOR --finding FINDING`. The user replaces the claim and sighting ids. `--json` is the same page and has no score field. Findings `c` lists stored claim ids. Enter reads one. The page writes nothing and starts no playback.

## Evidence

`cargo verify` exited 0 in 559.39 seconds. The `sigy` binary tests passed 384, with 7 ignored, in 31.74 seconds. The `sigy-service` library tests passed 762, with 2 ignored, in 225.52 seconds. `cargo audit --deny warnings` loaded 1351 advisories and scanned 382 dependencies. Coverage and `cargo visual-qa` were not run. `cargo verify-media` was not required. External spend is USD 0. This receipt was written after those commands.

## Limits

Issue 14 stays open. A passage that is not a stored finding is still refused. The stored relation labels are still the user's words, not behavioral fixtures. Phases that need news items or a measured model are not this slice. Issue 13 stays open. S-03B stays open. Process exit before a completion hold commits can still drop that outcome. No acoustic, Omarchy, language, or platform qualification is claimed. External spend is USD 0. This slice does not change recording, playback, or HTTP acquisition, so `cargo verify-media` is not required.
