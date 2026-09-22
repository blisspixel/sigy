# 0017: Local podcast subscriptions

Date: 2026-09-21. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

A podcast subscription is a user intent. It is not a radio favorite and not an `http_audio` source revision. This operation stores no feed text, and the stored URL grants no network, retention, or playback authority. Favorites stay Radio Browser identities. Directory cache rows and saved stations remain separate tables.

Catalog schema advances to v12. Local IPC advances to v13. Stop an older controller before replacing the executable. Migration from v11 creates an empty `podcast_subscriptions` table without changing budgets, sources, captures, directory rows, or favorites. A failed migration rolls back.

`podcast subscribe ID --url URL` stores the normalized feed URL, network scope, optional address pin, and redirect policy. Those four values are immutable for that id. Exact replay returns the stored row and does not restart polls. A changed URL, scope, pin, or redirect policy is a conflict and needs a new id. Validation reuses the existing HTTP authority checks: schemes, credentials, fragments, whitespace, literal-address classes, and the rule that public redirects require public-internet scope. Parsing a URL does not resolve DNS. Subscribe does not construct an acquisition request, register a source revision, or create a capture.

Ordinary views show the origin, scope, pin, redirect policy, and poll state. Paths and queries stay in the private catalog. Do not put access credentials in a feed URL. Userinfo is rejected.

The catalog admits at most 1,024 subscriptions, including stopped rows, and serves pages of 1 through 32. Only one active subscription may store a given normalized feed URL. A second active id for that URL is rejected.

`podcast unsubscribe ID` sets polls from active to stopped. The row remains. Repeating it has no further effect. Subscribe on that same id does not start polls again. A new id may subscribe to the same feed after the previous one is stopped. SQL rejects changes to the authority columns, reactivation of polls, and deletion of the row. Unsubscribe does not delete favorites, source revisions, captures, or directory cache rows.

Future feed refresh must skip stopped rows. Refresh is not implemented here. No RSS parser, episode list, enclosure download, transcript fetch, or automatic poll runs in this operation. The list explorer does not subscribe.

Verification covers immutable replay and conflict, denied destinations, redacted ordinary views, an unresolved hostname stored without a capture or audio revision, unsubscribe retention of an existing source and capture, a favorite left in place by subscribe, the 1,024 row ceiling, v11 migration rollback, offline CLI use, and the same commands through a running controller across restart. This does not fetch a feed, play an episode, or exit stage 4. No application dependency was added.
