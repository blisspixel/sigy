# 0104: Manual claims over stored findings

Date: 2026-10-21

Status: implemented user-written claims linked to findings that are already stored. News fetch, an anomaly record, automatic status, a truth or credibility score, and a stage exit are not claimed. Catalog schema is v52 and local IPC is v54.

## Decision

A claim exists only after an explicit command. The user writes the text, links a stored finding, attaches evidence, and sets any status. Feed text, transcript text, a monitor proposal, and a model cannot insert or revise a row. A text revision appends. It does not erase an earlier revision. Create of the same id and the same first text changes nothing. A different text for that id conflicts and writes nothing.

A sighting cites one existing finding. The finding already names the transcript revision, the translation cue, and either the cue's retained interval or an explicit expired or missing statement. A passage match that is not a stored finding is refused and writes nothing. The same sighting id and finding changes nothing. The same id with a different finding conflicts. A newer transcript or translation is reported stale on read and does not rewrite the sighting.

Evidence carries a type (`supports`, `disputes`, `context`, or `unresolved`), origin `user`, a note, and a relation the user chooses. The stored relations are `quotation`, `negation`, `sarcasm`, `changed-date`, `translation`, `shared-press-release`, and `correction`. Omitting the relation stores `unspecified`. These labels are the user's words. They are not a classifier. Storing evidence does not set a status.

Unverified is the absence of a status row. It is not inserted at create. A status change requires an evidence link that is already stored and records setter `user`. The expected revision is the count of status rows already stored, and zero means none exist. The token `retracted` is shown as "retracted or corrected by source". A later status appends. It does not erase the earlier one. No status is chosen by the service.

The page states its scope before the first sighting. This phase searched linked findings only. It did not search the library. Languages are not assessed outside the cited cues. The window is the cited retained intervals, or `none` when no retained interval is linked. The gap sentence says material outside the linked findings was not examined. Each sighting names its time basis: `recording media clock` for a retained interval, and `no retained interval` for an expired or missing finding.

Carriers group sightings by source revision. Repetition and independence stay `unknown`. Grouping is not corroboration. The page has no score field and does not read an anomaly record. There is no anomaly table.

One library stores at most 256 claims. One claim stores at most 64 text revisions, 32 sightings, 64 evidence links, and 32 status revisions. Text is at most 2,000 characters. A note is at most 500 characters. Empty text, control characters, and a negative clock are refused.

## What this does not do

This slice does not fetch, scrape, or store a news item. It does not add a globe layer, an anomaly lens, or a credibility score. It does not set a status from wording, from a model, or from a repeated carrier. It does not add an MCP tool that creates, revises, links, or sets a claim. `claim_list` and `claim_show` only read. It does not add pause, seek, or segment handoff. It does not close issue 14: later phases still need news items and a measured model. It does not exit a roadmap stage. S-03B stays open. Process exit can still drop an uncommitted recognition or translation completion.

## Interface

`sigy claim create`, `revise`, `list`, `show`, `sighting`, `evidence`, and `status` are the commands. Human output prints the timeline before the sightings. `--json` prints the same page, with the timeline field before the sightings, and with no score. The explorer cannot send a claim operation. Backup copies the new tables with the rest of the catalog.

## Evidence

`cargo verify` exited 0 on this Windows host. It passed vendor and script checks, the changelog heading check, formatting, warnings-denied Clippy, workspace tests, the build, and `cargo audit --deny warnings` against 1348 advisories and 382 dependencies. The `sigy` binary tests passed 362, with 7 ignored, in 25.60 seconds. The `sigy-service` library tests passed 744, with 2 ignored, in 183.53 seconds. The command completed in 448.99 seconds. Coverage was not run. `cargo verify-media` was not required. `cargo visual-qa` was not required. External spend remains USD 0. This receipt was written after those commands.

Storage tests keep wording history, refuse a sighting that is not a stored finding, report a stale citation on read, require a user evidence link before a status row, print the timeline and unknown carrier independence before the sightings, and store the named relations without a score. Caps stop at the declared bounds. A v51 catalog gains the claim tables on reopen. A conflicting `claims` table rolls back and leaves user version 51. Older fixtures drop empty claim tables before they replay. A populated claim is not downgraded.

The prior docs-only commit `4ca0578` had hosted verify on Windows, Ubuntu, and macOS: https://github.com/blisspixel/sigy/actions/runs/39649677344. That run does not include this slice. Hosted verify of `2623de0` passed on Windows, Ubuntu, and macOS: https://github.com/blisspixel/sigy/actions/runs/39686724967. The Windows job runs `cargo verify`. Ubuntu and macOS run build and lint. A clean detached worktree install wrote `installed-commit` `2623de03f34780a299139afbad5ac34d726e40fb`, 42 bytes with the SHA and a CRLF. `sigy --version` prints `sigy 0.1.0-dev`. `sigy claim --help` lists create, revise, list, show, sighting, evidence, and status. The install used that checkout and left `SIGY_SRC` unset. This later receipt does not change the program, so the installed binary remains that commit. Full `cargo verify` was not re-run for this receipt. Issues 13 and 14 stay open. No stage exit is claimed.
