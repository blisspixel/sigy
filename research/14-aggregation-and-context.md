# Aggregation and persistent context

Reviewed: 2026-09-20. Status: product research and independent design proposals. No third-party source code or visual assets incorporated.

## Relevant categories

Sigy combines discovery, continuous collection, decoding, analysis, and an evolving understanding of a topic. Existing tools provide useful examples of individual responsibilities without determining Sigy's implementation.

| Category and reference | Useful evidence | Implication for Sigy |
| --- | --- | --- |
| Radio directory: [Radio Browser](https://docs.radio-browser.info/) | Searchable station metadata and changing stream endpoints | Separate discovery records from actual observed source health and content |
| Feed aggregation: [FreshRSS](https://www.freshrss.org/) | Self-hosted aggregation with search, filters, and feed organization | Saved searches and collections can remain useful alongside automatic monitoring |
| Multiuser receiver: [OpenWebRX](https://github.com/jketterl/openwebrx) | Receiver sharing and multiple demodulation/decoding capabilities | Model scarce device capacity separately from the number of viewers or analyses |
| Extended receiver/decoder workspace: [OpenWebRX+](https://github.com/luarvique/openwebrx) | Additional digital decoders, recordings, and varied decoded outputs | Support typed artifacts and views beyond audio and text |
| SDR suite: [SDRangel](https://github.com/f4exb/sdrangel) | Broad native SDR device and processing ecosystem | Investigate maintained integration boundaries before recreating complex DSP |

These are conceptual and integration references, not dependencies or a promise to reproduce their feature lists. A project's implementation language does not select Sigy's language. OpenWebRX's AGPL and other upstream component licenses require separate review before source reuse, linking, bundling, or deployment decisions. Apache licensing of Sigy does not relicense another project.

Podcast ingestion and supplied resources belong to the first-release radio experience; broader RSS/Atom text analysis follows in stage 9. Their finite media, content revisions, polling and evidence contracts are developed in [Podcasts and feeds](19-podcasts-and-feeds.md). Additional source families remain independently scoped. A web receiver's existence also does not imply permission or an API for unattended recording.

## Proposed persistent topic notebooks

A monitor should accumulate useful context rather than produce disconnected summaries. A topic notebook can hold a current overview, a dated timeline, entities and aliases, unresolved questions, competing reports, saved queries, and linked evidence. Users should be able to open a claim and replay the supporting passage or inspect the originating packet/event.

Keep four distinct kinds of state:

| State | Update behavior | Authority |
| --- | --- | --- |
| Collected evidence | Immutable retained objects, with explicit retention/deletion records | What Sigy actually received |
| Derived knowledge | Versioned findings, links, summaries, annotations, and indexes | Revisable interpretation of evidence |
| Operational checkpoint | Current monitor progress, queue cursors, leases, and pending work | Transactional service state |
| User policy and curated context | Explicitly saved goals, source limits, corrections, terminology, and permissions | Governs allowed operations; model text cannot rewrite it |

A current overview is a projection with a history. Updating it must not erase earlier claims, contradicting passages, or the collection conditions that produced them. User corrections remain separately identifiable and survive automatic refresh. Proposed model changes to curated context need an explicit scoped editing workflow.

Entity links are useful when they preserve ambiguity. Similar names, aliases, transliterations, and recurring programme titles should produce candidate relationships with evidence. Do not merge identities solely because a model asserts equivalence. Separate reported claims from assertions Sigy can directly verify, such as capture time and measured duration.

## Storage and portability

Markdown and structured exports are useful for ownership, review, and use with other tools. They do not replace the transaction guarantees needed for concurrent jobs, paid reservations, evidence revisions, or recovery. A graph database is not automatically necessary for linked records; compare actual query and maintenance needs in the storage trade study.

Export a selected notebook with stable IDs, original-script names, source/time references, revision metadata, and explicit unavailable-evidence markers. Rebuild derived indexes from retained inputs where possible. If source material has expired, keep an honest record of what can and cannot be reproduced. Privacy-driven deletion must also address derivatives and external destinations according to their policies.

## Evaluation

Test a topic across repeated collection windows, an ASR correction, a changed entity match, a contradictory report, a model upgrade, expired media, a user annotation, and a restore. The notebook should remain understandable and its material claims traceable. Evaluate retrieval and claim support against actual recordings, not merely consistency with its own earlier summary.

The first release needs a useful monitor overview, history, and evidence navigation, alongside its podcast ingestion and supplied-resource workflows. Advanced graph exploration, cross-user collaboration, external knowledge synchronization, and broader syndicated text processing remain later decisions. See [Analysis and knowledge design](../docs/planning/10-analysis-and-knowledge.md).
