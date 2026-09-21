# 0006: Service-owned directory refresh and local search

Date: 2026-09-21. Status: initial implementation; validation evidence belongs in [progress](../development/progress.md).

This records the v5 discovery checkpoint. [Authorized redirects](0007-authorized-redirects.md) subsequently adds explicit source policy and advances catalog/IPC to v6; metadata refresh itself still rejects redirects.

[Radio favorites](0008-radio-favorites.md) adds user-owned saved stations and filtered search in catalog/IPC v7.

`radio refresh ID` admits one explicit metadata request through the persistent controller. Closing the CLI does not own the work. Reusing the same ID and parameters returns its state without another request. Restart marks unfinished refreshes interrupted; it does not resume them implicitly. The existing worker supervisor, HTTP destination policy, SQLite owner and IPC carry this work.

Catalog and IPC versions are v5. Stop older controllers before updating. Search, show and cache status are local operations available without the service. Refresh needs the running service. Station selection through `radio add` registers an immutable source revision without connecting to its stream; registration and directory provenance commit together.

The initial provider is Radio Browser. Default mirror discovery uses its SRV records with validated public HTTPS destinations. An explicit mirror supports self-hosting and tests; a pinned IP is scoped to that request. A refresh reads one page with 1 to 500 rows, offset at most 100,000, at most 2 MiB and 25 seconds overall. Individual HTTP reads have an 8-second deadline. At most two discovered mirrors are tried, with a delay between attempts. Requests share the existing two HTTP/DNS slots with capture.

There is one active refresh, a two-second minimum admission interval, at most 4,096 historical requests and 10,000 cached station identities. Cache capacity failure rolls the complete page back. Pages expose accepted and rejected counts. Invalid rows are skipped; duplicate identities or malformed page structure reject the page. Unseen stations remain cached, because a bounded query cannot prove deletion upstream. Each station keeps its observed time and refresh ID. Directory-reported health and languages are not observed playback health or audio-language detection.

Local search provides Unicode lowercase name matching, country, exact language/tag labels and directory health filtering. Pages contain at most 16 stations within the bounded IPC envelope. Metadata preserves scripts and rejects terminal control characters. Full stream paths/queries are retained privately for explicit registration; ordinary views expose origins. Station source revisions always have public-internet scope, even when discovered through a privately hosted mirror.

No clicks, votes, images, station streams, models or paid endpoints are contacted by refresh/search/registration. Integrated playback and its provider telemetry policy remain separate work. There is no automatic periodic refresh yet, no complete-world cache claim, no full linguistic collation and no second directory provider. See [directory research](../../research/27-radio-directory.md) and the [explorer contract](../planning/11-radio-explorer-and-dvr.md).
