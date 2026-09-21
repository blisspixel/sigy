# Internet radio discovery

Reviewed: 2026-09-20. Status: desk research and a read-only directory probe.

## Findings

Radio Browser exposes searchable station records including stable station UUIDs, country codes, language, tags, stream URLs, codec/bitrate, health-check metadata, and optional geographic coordinates. Its documented click metrics describe interactions with the directory. They are not audience measurement. [API reference](https://docs.radio-browser.info/).

The service recommends discovering API servers through DNS, randomizing the list, failing over on request failure, using an identifiable application user agent, and using station UUIDs rather than old numeric IDs. It also documents recording deliberate playback clicks. A permanent dependency on one named mirror would contradict the integration guidance. [Integration guidance](https://api.radio-browser.info/).

Geographic discovery, current station catalogs, and persistent recording controls are specified in [Terminal geography and radio DVR](18-terminal-explorer-and-radio-dvr.md). The intended experience uses independent design and qualified directory data.

## Proposed approach

Use directory adapters rather than embedding a directory's schema throughout the product. Radio Browser is a strong first candidate because it exposes the discovery fields needed by the product. Preserve its UUID as an external identity alongside Sigy's own source ID.

Maintain a local cache with retrieval timestamps and provenance. Favorites and user collections survive a directory outage. Search uses bounded requests and pagination. Country, spoken language, station location, and topic are separate fields.

Separate directory health from local observations. Store the submitted URL, resolved URL, redirects, and observed codec without treating every redirect as a new station. Avoid logging credentials embedded in URLs.

Allow user-entered public stream URLs and explicit playlist imports. Resolve nested playlists with depth, size, protocol, and timeout limits. Do not automatically crawl arbitrary sites to find hidden streams.

Use shared acquisition for compatible jobs listening to the same stream. Record directory playback clicks for deliberate user playback according to documented expectations; do not let automated source probing manufacture directory popularity. Automated-monitor accounting behavior needs an explicit integration decision.

## Alternatives and tradeoffs

| Approach | Benefit | Cost or limitation |
| --- | --- | --- |
| Public directory | Broad discovery and structured filters | Third-party uptime, incomplete metadata, stale stream URLs |
| Curated built-in catalog | Consistent presentation and tested examples | Ongoing maintenance, narrower coverage |
| Direct stream entry | Independence from directory coverage | Less metadata and user-managed correctness |
| Multiple directories | Better resilience and coverage | Identity reconciliation and conflicting metadata |

A directory plus direct URLs is the leading proposal. A curated set can support onboarding and regression testing without becoming the sole catalog.

## Unvalidated work

Build a dated sample spanning countries, languages, codecs, HLS/direct streams, redirects, and broken links. Measure resolution and playback success separately. Check metadata completeness, coordinate validity, duplicate URLs, source naming, and retrieval behavior during mirror failure.

Define cache lifetime and refresh policy from practical provider behavior. Confirm station-directory use terms and any redistribution obligations for a packaged catalog before shipping it.

Refresh needs reconciliation of renames, URL changes, missing records, partial pages, and mirror disagreement without losing favorites or rewriting old recordings. Recently changed listings are not assumed to be a complete ordered update log. Directory freshness, local stream health, and programme metadata are distinct.

## Near future

Keep directory adapters replaceable. Enrich geographic exploration only when coordinates have sufficient quality, and label unknown locations. Do not depend on a single external globe application or copy its assets or station catalog as an implementation shortcut.
