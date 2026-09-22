# Podcasts, RSS, and Atom for automated insight

Reviewed: 2026-09-20; priority updated 2026-09-21; implementation status updated 2026-09-22. Podcasts are prioritized with internet radio before hardware. Broader RSS and Atom text analysis remains later work. Local subscribe, one bounded RSS 2.0 refresh, one explicit enclosure download, retained playback of that file, and one explicit publisher transcript or chapter fetch are implemented. [Follow-up research](26-recording-discovery-and-rf.md) records a bounded publisher-feed inspection and corrected namespace semantics.

## Fit

Podcasts and feeds extend monitoring beyond live radio. A topic monitor could follow broadcasts, new podcast episodes, and published text, then compare what each source reports. They should reuse Sigy's source identities, durable jobs, language handling, evidence, local processing, and spending policy. They need acquisition semantics appropriate to finite and revisable documents.

## Primary-source findings

RSS 2.0 represents channels and items, with optional item identifiers, publication times, and media enclosures. Item descriptions can contain a synopsis or the content itself; a link does not establish that the full article is included. [RSS specification](https://www.rssboard.org/rss-specification).

Atom distinguishes stable entry identifiers, update/publication times, content, summaries, and link relationships. Preserve those distinctions when normalizing into Sigy's model instead of flattening everything into a URL and text field. [RFC 4287](https://www.rfc-editor.org/rfc/rfc4287).

Apple's podcast publishing requirements describe episode identifiers, enclosures, and HTTP delivery expectations. They are useful interoperability fixtures, not a requirement that every feed use one directory or satisfy all of a publisher platform's rules before Sigy can read it. [Podcast RSS requirements](https://podcasters.apple.com/support/823-podcast-requirements).

Podcasting extensions include additional episode information such as transcripts and chapters. Treat support as capability-specific, optional enrichment with provenance and validation, not an assumed feature of every podcast. [Podcast namespace documentation](https://github.com/Podcastindex-org/podcast-namespace/blob/main/docs/1.0.md).

HTTP semantics define validators, conditional requests, ranges, and response codes useful for efficient polling and recoverable media downloads. A server may not support all of them. Resume only when the response and object identity make it safe. [RFC 9110](https://www.rfc-editor.org/rfc/rfc9110.html).

## Proposed acquisition contracts

| Source/object | Acquisition | Evidence identity |
| --- | --- | --- |
| RSS/Atom subscription | Bounded periodic conditional fetch and reconciliation | Subscription identity, retrieval snapshot, entry ID and content revision |
| Podcast episode | Admitted finite media download or supported playback; optional supplied transcript | Episode identity plus the exact media version and media-time intervals |
| Feed text | Retain included content/summary, normalize safely, preserve original | Content revision with paragraph/character anchors |
| Linked article | Separately permitted bounded fetch through a qualified adapter | Retrieved representation, canonical/source URL evidence and fetch time |

Start with user-supplied feed URLs and selected subscriptions. Discovery adapters and subscription-list import/export are later refinements; no particular podcast directory is required by this design. Do not automatically crawl all linked sites or fetch every historical episode.

RSS identifiers can be absent or malformed, publishers can reuse identifiers, and URLs can move. Prefer publisher IDs within source scope, then a documented fallback based on stable metadata and content evidence. Do not use titles alone. Record uncertain duplicates and migrations rather than silently merging distinct episodes. Preserve both duplicate acquisition suppression and editorial relationships between syndicated stories.

An existing episode URL can later serve different bytes because of editing or dynamic advertisements. Keep the acquired object's hash and revision so timestamp citations refer to the actual audio analyzed. Download retries need validator/range checks; a resumed transfer must not concatenate two versions.

## Local analysis and mixed-source evidence

Reuse provided transcripts only with origin, language, alignment, and completeness recorded. They may differ from Sigy's fetched audio; qualify alignment before claiming precise replay. Otherwise queue local ASR and translation using the same live/batch resource controls. Initial podcast ingestion can prioritize finite batch processing; a live podcast extension is separately scoped.

For text, detect language per relevant block and retain original content alongside translations. Feed language and episode language are hints, just as station language is. Summaries, show notes, advertisements, quoted text, and complete editorial content need distinct roles in analysis.

Classify new or materially revised evidence, retrieve relevant passages, group related reports, and update topic context. Counts remain deterministic. A radio segment, podcast episode, and article are different units; a combined trend must declare its measure and denominators instead of adding incomparable counts into a popularity score.

Record publication time, update time, first observation, fetch time, and media time separately. Older episodes newly discovered today are not automatically new events today. A feed with a short rolling history cannot prove that Sigy saw every publication while it was offline.

## Scaling and controls

Use bounded polling with validators where available, provider/host concurrency limits, backoff, jitter, and explicit freshness targets. New subscriptions preview backfill count, download bytes, media hours, storage, and estimated processing before admission. Set finite defaults for historical import.

Persist per-subscription cursors and fetched snapshots, then commit normalized revisions and jobs atomically. Repeated polls should not repeatedly invoke models on unchanged content. Content/model/taxonomy revisions determine reusable derivatives. Large feeds and long episodes must not starve live radio work.

Show fetched, retained, queued, processed, excluded, unknown, and failed coverage. A failed poll does not remove subscriptions or erase historical evidence. Unsubscribing stops future work; removal of retained evidence is a separate operation governed by policy.

Every remote classifier, recognizer, translation, or synthesis request uses the shared cost ledger. Feed access and downloads also need network/storage limits even when inference fees are zero. Account-based podcast services, private tokenized feeds, paywalled content, and paid directories need separately configured access and terms; no such integration is assumed.

## Input boundaries

Use bounded XML parsing without external-entity resolution, bounded decompression, safe handling of embedded HTML, and strict limits on redirects, enclosures, and retrieved documents. Never interpret feed or transcript instructions as monitor permissions. Prevent nested URLs from bypassing the existing network policy. Redact private feed tokens from display, exports, and diagnostics.

An accessible feed is not permission to redistribute articles or episodes. Preserve content rights metadata where available, keep collection and publication separate, and apply the existing lawful-use and export policies.

## Qualification and unresolved decisions

Specify RSS/Atom version and namespace coverage, the allowed linked-content scope, private-feed support, transcript/chapter formats, initial directory/import behavior, polling bounds, and backfill/retention defaults before implementation.

Later tests need multilingual text/audio, summary-only feeds, missing/reused IDs, moved feeds, updated episodes, dynamic advertisements, stale validators, truncated downloads, range refusal, oversized XML, unavailable media, replay alignment, syndicated content, and restart recovery. Measure useful topic coverage and queue fairness under mixed live-radio and feed workloads. No general web crawler or distributed collection fleet is required for this roadmap milestone.
