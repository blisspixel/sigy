# Ordered worldwide discovery validation

Date: 2026-10-03. Status: EX-01C passes local Windows verification. No roadmap stage exit, supported release, language quality, native font, hardware or capacity qualification is established. [Active work](../../docs/development/progress.md) owns current state; [decision0081](../../docs/decisions/0081-ordered-station-search.md) owns the contract. Source-only publication requires exact-commit hosted CI; [checkpoint metadata](https://github.com/blisspixel/sigy/releases/tag/v0.1.0-dev.20261003.1) records the commit and run outcome.

## Scope and lineage

Starting source checkpoint is `1208ea7a51b5863dfa2ca9a1e593946ae989e1d7`, the earlier source-only workstation milestone. This increment advances catalog schema from v47 to v48 and local IPC from v48 to v49. MCP remains 2026-07-28. No dependency, native vendor, country reference, coastline asset or required legal notice changes.

The same Windows 11 x86_64 workstation uses Ryzen 7 7840U, about 64 GiB RAM, Rust 1.98.1, two Cargo jobs and two ordinary test threads. Native media fixtures run on one thread. GPU processing is not exercised. Ordering/fault fixtures are synthetic; retained real station metadata is used only for offline console navigation. No directory refresh, public stream recording, station probe, model inference or hardware attachment is performed. New external spend is USD 0 of the cumulative USD 20 ceiling, excluding the development harness.

The current source map covers 396 Rust/SQL/manifests, build configuration and compiled fixture/asset files. Its private receipt is `.agents/ordered-blob-source-hashes-20261003.json`, SHA-256 `de497a8147030214c264b9f9dff053f54620f84bafe8db4657ee753f3977603e`. Separate asset/license hashes are checked unchanged. The preceding TEXT-key source map and gate receipts remain historical; they are not the final BLOB-key verification.

## Comparison, faults and migration

An independently declared 37-station sequence traverses pages of 16, 16 and 5. It includes duplicate full-fold keys, composed/decomposed accents and Arabic, Devanagari, Cyrillic, Greek, Han and Korean labels. UUIDs resolve equal keys. Originals remain exact. Other fixtures cover actual/no-op favorite revision changes, failed refresh, renamed names, malformed/stale/cross-query cursors, an existing out-of-filter continuation tuple, bounded history and delayed replies. Refusal preserves prior applied scope, selected source and rows; explicit `g` starts cached page one.

Review found that BINARY TEXT comparison depends on SQLite's database encoding. The repair stores and binds normalized UTF-8 BLOB keys, independent of database text encoding. UTF-16LE and UTF-16BE fixtures publish and page an independently declared order before and after populated v47 migration. A supplementary Deseret letter and a private-use code point exercise a real UTF-16BE-versus-UTF-8 ordering reversal. Exact station metadata and favorites survive. Separate hostile cases refuse TEXT keys, invalid UTF-8, oversized BLOBs and valid UTF-8 keys inconsistent with the name. Reopen refuses corrupt keys; restoring the declared valid bytes restores reads without rewriting metadata.

Populated migration failure leaves v47 and no new column; removing the injected DDL conflict permits reopen/migration. Verified restore renews only disposable namespace/revision, rejects an old cursor and preserves source hashes, favorites and canonical history. Historical checkpoint/task migrations use explicit test-only removal of the newer projection when rehearsing earlier versions.

The ordered read includes existing control-wrapper ledger/budget inspection in one consistent transaction and the unchanged 100 ms cooperative SQL guard, four-million-VM-operation ceiling and 10 ms lock wait. A continuation is at most 8,192 ASCII bytes. At most 17 SQL rows provide lookahead and at most 16 station payloads are decoded. Raw metadata is checked against 8,192 bytes before copying. Capped page and complete control-frame writers reject overflow before growth or wire header/body output; one exact-limit wire case and an early-stop million-element case exercise the boundary. These mechanisms cannot force blocked filesystem calls to return.

CLI UUID ordering remains the default; `--order name` explicitly selects the new read. The TUI uses the same name-ordered operation. The existing read-only MCP `radio_search` accepts optional exact `id`/`name` order, validates the 8,192-byte cursor separately from unrelated 2,048-byte strings, and advertises the actual 1-to-16 station bound. No new tool or mutation authority is introduced.

## Bounded query observations

The 10,000-station fixture publishes twenty bounded 500-station batches. Its independently permuted station names/UUIDs and 32 legal tags avoid insertion-order agreement. `EXPLAIN QUERY PLAN` uses `directory_name_order` without a temporary sort. The focused final BLOB run measured the complete control-wrapper first page at 2.1486 ms, a sparse name query at 13.1997 ms with 100,000 checkpoint VM operations, and an absent-tag query at 46.0229 ms with 1,200,000 operations. Both filtered queries succeeded. The fixture also accepts explicit cooperative-work exhaustion on a slower run, never an unbounded fallback. Interrupt cleanup preserves a separately configured 37 ms lock wait and a subsequent valid read.

These are single debug fixture observations on this workstation, without latency distributions, capture-load measurement, Pi/NAS evidence or a supported capacity promise. They do not justify increasing limits or replacing the catalog.

## Interface evidence and repairs

The initial full check found five interface failures: an added line reduced the 80x24 station list from ten rows to nine, and compact/editor help hid active controls or quit. The repair places catalog context on the existing search line and retains explicit compact restart guidance. Assertions and row capacity were preserved. Formatting/lint repairs split oversized functions and boxed large futures; no lint was allowed away.

Production test-backend frames cover normal catalog context, stale rows and stale filter editing at 20x8, 40x12, 80x24, 132x40 and 160x50. The final rebuilt test binary exports 15 current frames; six are rasterized and representative compact/wide views inspected. Their synthetic source labels and activity do not establish a native-script station observation or actual terminal player. Rasterized views remain separate from actual console-cell evidence.

The clean final console run uses independent empty and retained 16-station libraries at 20x8, 80x24 and 132x40. All six clients exit zero at the requested dimensions. The receipt preserves 48 real cell/attribute frames and 30 checked key/frame transitions across cached restart, filter editor, 257-country picker, globe and return to the list. All 16 IDs in the wide retained page agree with the actual CLI name-ordered page, including its first selected station. Eight complete before/after inspections, radio-status/recordings/schedules/doctor for both libraries, remain equal. No capture, network, policy or budget state changes.

An initial driver run lost its command-output handle after detaching from a console. Repairing inspection alone left initialization on that detached handle; the second failure exposed this remaining launcher path. All driver commands now own separate redirected pipes, start without a visible window and stop only their owned process on deadline. The final run completes all six cases without resume. These were driver defects, not application failures. Earlier failed/resumed receipts remain historical. Native pixel capture was unavailable; no native font, glyph-shaping, SSH or key-to-frame latency qualification follows.

The final executable SHA-256 is `19463089c0e58da5e17809d23762b0239b59bfbf2ad940f384d82468e36d9b88`. The source and copied retained-cache hash is `5d774dc17735227fa9fd3d5c32e851fd51bdc7c2319a008969c0b3a261f6b7aa`. The final native receipt `.agents/ordered-native-20261003/20261003T205502Z/validation-receipt.json` has SHA-256 `378ed49e4327e9af152704737e5b13b4384538f85e3a6f4ef3c0753a9143c059`.

## Gate receipts

| Gate | Outcome on repaired source |
| --- | --- |
| `cargo verify` | Passed: 944 ordinary test executions, 17 ignored, formatting, warnings-denied Clippy, build, native-source hashes and advisory audit of 313 dependencies against 1,290 advisories |
| `cargo verify-media` | Passed: all 16 fixtures, 110.91 seconds of native execution |
| `cargo verify-coverage` | Passed: sigy 88.20%, sigy-core 94.41%, sigy-service 93.14%, sigy-test-recognizer 90.81%, sigy-xtask 93.00%; no source exclusions |
| Native console navigation | Passed: six sessions, 48 cell/attribute frames, 30 checked transitions, eight unchanged inspections and 16 matching CLI/TUI page IDs |
| Documentation | Passed: 29 changed/new Markdown files, 857 local links/anchors, complete registers, 27 packages, writing rules, README legal/setup preservation and `git diff --check` |
| Exact-commit hosted CI | Required before source-only checkpoint publication |

The final coverage JSON is `target/coverage-reports/workspace-34124-1791061006834145200.json`, SHA-256 `ddab8c41bff9220a681080f89e9b7da7d3a5d1c8cc194ca3c2a7f3f0a6f26609`. Exact covered/executable lines are sigy 20,136/22,829; core 1,436/1,521; service 53,854/57,817; test-recognizer 257/283; xtask 1,130/1,215. Ordinary tests use two threads, ignored measurement/native fixtures one, with test sources and build scripts included. This is compiled Windows line coverage, not branch/platform qualification.

Final command-log SHA-256 values: verify `a67b924ff2f0f7e90f30f6fdd4b63a3eb07bdf249342d47b1fda17bcb2b68b9e`; media `cdf73ec07dd250ed275d621fb71cdb55bde7458a39c3c0dbad3996bc714b7d6a`; coverage `aa1005bd08687d1a74ebfcf77960ef782b06f3f67dbe7b4b611379e99092f7ee`. Source and 14 retained asset/license hashes remain unchanged through the gates.

The first TEXT-key tree passed 942 ordinary executions, all 16 native fixtures and every crate's coverage gate before final review found the encoding invariant. Those receipts and earlier compiler, lint and interface failures are retained privately. The final source adds UTF-16/malformed-key regressions and requires current receipts. Dated external [listening and receive-only research](../42-calm-listening-and-receive-only-sources.md) and [near-term briefs](../../docs/development/near-term-implementation.md) describe the next focused inspection, protected player and typed replay increments. They establish no current TUI player, SDR/LoRa adapter or additional translation target.
