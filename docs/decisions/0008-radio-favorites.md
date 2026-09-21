# 0008: Favorites belong to the user

Date: 2026-09-21. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

Directory refresh must not overwrite saved station choices. Store favorites by provider and station identity in a separate catalog table, not in provider metadata. The first provider remains Radio Browser. `radio favorite ID` and `radio unfavorite ID` set the desired state; repeating either operation has no further effect. These operations require a valid cached identity and never contact a stream, register a source, schedule recording or report a provider vote.

`radio search --favorites` combines the selection with existing name, country, language, tag and health filters before pagination. `radio show` and search pages include bounded `favorite_ids` independently of directory observations. Status includes the current favorite count. Both commands work through the controller or through the same operations with the service stopped.

The catalog and local IPC advance to v7. Stop older controllers before replacing the executable. Migration from v6 creates an empty favorites table atomically without changing source, recording, budget or directory rows. A composite foreign key with `ON DELETE RESTRICT` prevents cache deletion from erasing a saved identity; foreign keys are already enabled on the canonical connection. [SQLite constraint semantics](https://www.sqlite.org/foreignkeys.html#fk_actions) reviewed on 2026-09-21. No dependency changes are needed.

Refresh updates the displayed metadata for an existing identity, while the favorite remains. Omission from a partial page is not removal upstream. Existing immutable source revisions keep their authorized URLs even when the directory changes; recording still requires separate source selection. Unfavoriting removes only the preference, preserving cached metadata, sources and recordings.

The existing 10,000-station cache bounds favorite cardinality; response pages remain at most 16 entries. Future cache eviction must preserve saved rows or introduce a deliberate migration that retains user state. Collections, custom labels, station-identity reconciliation across providers and automatic refresh remain later work. Do not infer a working stream from a favorite or from directory health.

Verification covers persistence across refresh/reopen, changed URLs, omitted stations, repeated mutations, unknown identities, deletion protection, filter/cursor correctness, failed writes, migration rollback and real CLI use with the controller running and stopped. This increment does not establish Linux/macOS qualification, a TUI or integrated playback.
