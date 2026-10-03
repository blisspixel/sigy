# 0081: Revision-bound station-name paging

Date: 2026-10-03

Status: implemented bounded EX-01C slice with local Windows verification. [Validation evidence](../../research/experiments/ordered-discovery-2026-10-03.md) and active work record exact scope. This is cached discovery, not station validation, language qualification or a supported release.

## Decision

Add an explicit name-ordered directory read while preserving the existing UUID-ordered operation and CLI default. `radio search --order name`, the existing read-only MCP `radio_search` tool's optional `order: "name"` and the terminal explorer share the new operation. CLI/MCP omission retains UUID order. They read the existing service-owned cache and grant no refresh, station connection, click, recording or analysis.

Comparison uses the existing pinned Unicode 17 normalization data: NFC, full case folding, UTF-8 byte order and station UUID as the final tie breaker. Extract one bounded normalization implementation for country and station keys. Displayed originals, accents and scripts remain intact. This is a deterministic comparison policy, not locale collation, transliteration or popularity ranking. The existing lowercase filter semantics remain unchanged; cursors bind their actual effective strings rather than a different normalized interpretation.

Store comparison keys as UTF-8 BLOBs and bind their bytes for indexed paging and cursor membership. SQLite TEXT uses the database's UTF-8, UTF-16LE or UTF-16BE encoding, so text BINARY collation cannot establish this declared UTF-8 order across catalogs. The existing STRICT table enforces BLOB type; byte constraints and bounded raw-type/UTF-8 validation precede allocation. Reopen audits and returned-row validation compare keys with the shared derivation. [SQLite datatypes](https://www.sqlite.org/datatype3.html) and [encoding controls](https://www.sqlite.org/pragma.html#pragma_encoding), reviewed 2026-10-03, document the distinction. The retained native source's `binCollFunc` and three BINARY registrations confirm it; no vendor bytes change.

Schema v48 adds a disposable indexed name key and one catalog namespace/revision. Local IPC v49 adds `SearchOrdered`, its typed page and catalog metadata. Migration bounds and validates the existing maximum 10,000 stations before deriving keys. Successful directory publication increments the revision in its transaction. An actual favorite change increments it in the same transaction; no-op favorite requests and failed refreshes do not. Cache aging does not change membership. Restore into a new library renews only the disposable cursor namespace and revision, preserving canonical evidence, immutable source revisions and retained hashes.

An opaque continuation binds namespace, revision, comparison identity, effective name/country/language/tag/health/favorites filters, page-size policy and the last name-key/UUID pair. Verify that pair is still a member of its frozen query. Malformed, stale and cross-query cursors refuse rather than silently changing scope. A consistent deferred read transaction binds catalog identity, rows and favorite flags. This detects catalog drift; it does not preserve historical directory snapshots.

Use the indexed keyset within the existing 100 ms cooperative SQL-work guard, four-million-VM-operation ceiling and 10 ms lock wait. The control wrapper's ledger and budget inspection remains inside that guard and transaction. Resource exhaustion is a refusal, including a sparse query with no matches; returned-row limits alone do not bound work. These limits cannot force blocked filesystem calls to return and establish no portable latency claim.

Keep the existing 128-byte filters, 256-byte station names and 16 returned stations. Derived keys are at most 3,072 bytes; cursors at most 8,192 ASCII bytes and stored metadata at most 8,192 bytes checked before copying. Page encoding is capped before growth. Complete control-frame encoding independently enforces the existing 256 KiB transport limit before writing a frame header or body. A refused encoding therefore cannot leave a partial response on the stream.

The terminal keeps client request generations separate from catalog revisions. It retains prior rows, applied filters and source identity on refusal or stale replies, and offers explicit `g` to restart cached page one. A favorite response carries catalog metadata so delayed reads cannot restore an older revision. Page history is bounded. Browsing remains independent of explicit listening and background collection; the [calm session contract](../design/terminal-experience.md#calm-listening-sessions) adds no current terminal player.

## Alternatives and limits

Raw UTF-8 order would distinguish canonically equivalent names and case variants. Host locale collation would introduce an unselected platform dependency and inconsistent order. Fetching every station in the client would move catalog work and authority outside the service. Replacing legacy cursors would break existing scripts. Retaining historical catalog snapshots is unnecessary for this finite discovery slice.

Country names are already available offline across 257 entries and eight display locales. City lookup, larger bounded refresh, local health validation/repair, focused listening composition and protected DVR playback retain their separate packages. Hardware support follows typed file replay and actual receive-only device qualification. English remains the implemented translation target; neither this migration nor country localization expands language-quality support.

## Evidence gates

Traverse an independently specified 37-station order as 16/16/5, including duplicate folded names, canonical equivalents and native scripts. Check every original label and UUID, backward/reload behavior and CLI/TUI agreement. Exercise renamed stations, actual/no-op favorites, failed refresh, stale/tampered/cross-query cursors, restored namespaces and delayed responses. Test metadata, cursor, encoding and cooperative-work exhaustion with cleanup and a subsequent valid read. Rehearse populated v47 migration, rollback and reopen without rewriting source/evidence history.

Exercise UTF-16LE and UTF-16BE publication and populated migration against an independently declared order, including a supplementary Deseret letter and private-use code point that reverse their relative text-byte order in UTF-16BE. Preserve original metadata and favorites. Refuse TEXT keys, invalid UTF-8, oversized BLOBs and valid UTF-8 keys that disagree with the station name; restored valid keys must read again without metadata rewrites.

Inspect production renders and actual terminal keys at compact and wide sizes. Preserve capture, network and budget state during navigation. Run warnings-denied checks, `cargo verify` and `cargo verify-coverage`; record exact artifacts and limitations in [active work](../development/progress.md). Synthetic ordering fixtures do not qualify worldwide coverage, native fonts, audio playback, long-term recovery or host capacity.
