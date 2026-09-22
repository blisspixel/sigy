# 0018: RSS feed refresh

Date: 2026-09-22. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

`podcast refresh` reads one RSS 2.0 document for one active subscription. It uses the shared acquirer, the subscription's immutable network scope, address pin, and redirect policy, and the directory-refresh lifecycle: one active feed refresh, exact replay, and the last good snapshot kept. A stopped subscription is not polled. Refresh writes metadata only. It does not register an audio revision, reserve quota, or create a capture.

Catalog schema advances to v13. Local IPC advances to v14. Stop an older controller before replacing the executable. Migration from v12 adds refresh, snapshot, and episode tables without changing budgets, sources, captures, directory rows, favorites, or subscription authority. A failed migration rolls back.

The compressed body is at most 2 MiB. The decoded document is at most 8 MiB. The expansion ratio is at most 16. Supported content encodings are identity, gzip, and zlib-wrapped deflate. flate2 1.1.10 supplies that decoder through its pure Rust backend. Other encodings fail closed. The document deadline is the existing 8-second metadata deadline.

The parser accepts RSS 2.0 only. Depth is at most 32. A DTD, an external or undeclared entity, and `XInclude` fail the document. Only the five predefined XML entities and numeric character references are expanded. `xml:base` is ignored. Relative references resolve against the final fetched document URL, which already passed the subscription redirect policy. Feed text does not grant a new network scope.

At most 500 identifiable items are committed. Further identifiable items mark the snapshot truncated and are not stored. An item with a publisher guid uses that guid inside the subscription. A missing guid may be derived only from a normalized enclosure URL plus a parsed publication time, and that derivation is labeled `derived_enclosure`. A title never identifies an episode. An item with neither identity is skipped. The first copy of a guid in one document wins.

Omission from a later snapshot is not deletion. Episodes keep their first-seen order. A subscription retains at most 2,000 episodes so a rotating feed cannot grow the catalog without a bound. A new identity beyond that ceiling is not inserted, and the snapshot is marked truncated. The ceiling does not delete older rows.

Transcript and chapter URLs from the Podcasting 2.0 namespace are stored with the episode, at most 8 transcripts and 4 chapter documents. Further references mark the snapshot truncated. They are not fetched. A `podcast:liveItem` is counted and not opened: its enclosure and assets do not become an episode and are not requested.

A failed document, hostile XML construct, size or expansion limit, or unauthorized redirect marks the refresh failed and leaves the previous snapshot and episodes unchanged. Ordinary episode views show identity kind, title, publication time, whether an enclosure exists, and asset counts. They omit enclosure, transcript, chapter, and feed paths and queries.

The list explorer does not subscribe, refresh, or list episodes. No automatic poll runs. Enclosure download is a separate explicit operation. See [one episode enclosure](0019-episode-enclosure.md). This does not exit stage 4.

Verification covers parser fixtures for guid identity, derived identity, title-only items, duplicate guids, 501 items, depth, DTD, external entities, and `XInclude`; catalog omission, failed-document retention, replay, one active refresh, the two-second interval, the 2,000-episode ceiling, and v12 migration rollback; gzip and deflate bounds on the shared acquirer; and a local service fixture of 239 items with one derived episode, one skipped title, and one live item. That fixture contacts only 127.0.0.1. Replay does not fetch again. A private redirect fails closed. Offline listing shows the retained episodes and omits stored URLs.
