# Workstation milestone validation

Date: 2026-10-03. Status: the bounded-work repair passes integrated local gates after the first hosted run failed the largest publication fixture. Exact-commit hosted verification is required before source-only publication; [checkpoint metadata](https://github.com/blisspixel/sigy/releases/tag/v0.1.0-dev.20261003) retains its commit and run outcome. No roadmap stage exit, supported release, language pair, deployment or simultaneous-capacity qualification is established. [Active work](../../docs/development/progress.md) owns current state.

## Scope and inputs

Starting checkpoint `697a58e` contains the repaired native fixture, terminal cache filters and research/implementation briefs. Bounded reconciliation and country discovery are integrated at `6b82c81`, `7efba4d` and `cb5e0c4`; exact observations/publication and withdrawal are verified together on the resulting tree. Catalog schema is v47 and local IPC v48.

The workstation is Windows 11 x86_64, Ryzen 7 7840U, about 64 GiB RAM and Radeon 780M, with Rust 1.98.1. Cargo uses two jobs; ordinary tests use two threads and native media one. GPU processing is not exercised. Runtime fixture text and deterministic native children are synthetic. Native media uses loopback acquisition, generated/retained audio and bounded stand-in workers; it does not qualify recognition or translation quality. Country interaction uses separate empty and retained real 16-station metadata caches without a refresh or station contact.

External spend for this goal is USD 0 of the cumulative USD 20 ceiling, excluding the development harness. No paid allocation/request, public recording, inference comparison, archive dataset download or new model download is performed. The country reference imports pinned public CLDR/Unicode data with their required notices; it establishes neither worldwide station inventory nor interface-language qualification.

## Exact evidence and publication

[Decision0078](../../docs/decisions/0078-exact-task-evidence-publication.md) defines streaming reconciliation, immutable exact observations, disjoint publication origins and canonical briefing membership. Fixtures cover:

- UTF-8/type/per-value checks before allocation, cumulative byte/header limits, exactly 64 citations and a known 65th match, query interruption and cleanup, lock refusal and transaction rollback.
- Pending jobs frozen without waiting, explicit partial remainder, exact 65,536-byte encoding and refusal/truncation above the cap, shared 128-observation lifetime capacity and separate legacy/exact ordinals.
- Populated migration, interrupted migration, preserved historical JSON/hashes/receipts, scope/clock drift, exact replay before live checks and cancellation generation fences.
- Transcript corrections, translation completion, same-source unrelated recordings, missing media, whole-recording deletion and legal partial segment release without rewriting history.
- Hostile rehashed lineage, cue text, job state, media bytes, citation membership and false complete no-match; complete snapshots independently reconstruct bounded full ordered membership.
- At most 64 finding effects, original-only skipped outcomes, one lifetime run across both origins, exact successful finding membership, exclusive snapshot coverage origin, historical inspection and orphan-receipt rejection after trigger bypass/reopen.

Focused checks passed 38 snapshot tests, three query-work tests, 12 run-filter tests and eight exact-run tests. These filters overlap; they are not distinct workspace totals. Some early fixture/schema/helper and lint failures were repaired before the integrated gate. No assertion or lint was relaxed.

The shared work guard is 100 ms cooperative wall time, four million SQLite VM operations and 10 ms lock wait. Reconciliation examines at most 4,096 cue headers and 4 MiB original/English text, with 4,096-byte per-value bounds. Snapshots use a capped 65,536-byte writer. Failure refuses fresh effects or preserves an explicit unknown remainder. These are bounded failure envelopes; blocked filesystem calls can still outlast cooperative deadlines.

One debug storage fixture with 128 published cues and 64 matching findings measured admission at 21,231 microseconds, the maximum of 65 executor ticks at 54,182 microseconds and exact briefing read at 36,827 microseconds. A largest legal 512-cue reconciliation fixture examined 2,164,736 bytes over 16 runs, with a median of 9,355 microseconds and maximum of 12,438 microseconds. These are particular synthetic observations, without a throughput, tail-latency or machine-independent guarantee.

Historical media byte counts describe their observation time and are audited against the immutable published interval envelope. Existing release history does not independently reconstruct each earlier retained-byte instant. Sharing counts are historical observations; current authority comes from canonical interests. Today's target identity is explicitly `en`; non-English target execution remains S-05 work.

## Hosted resource failure and bounded-work repair

Hosted [run37138345510](https://github.com/blisspixel/sigy/actions/runs/37138345510) checked commit `4e91636d4170a44eefdb3dce53fff6ab3ec5c359`. It failed only `full_exact_membership_measures_guarded_admission_read_and_tick`, with SQLite operation interruption under the unchanged 100 ms cooperative guard; 601 other service tests passed and one was ignored. Initial local success did not predict that runner's resource behavior.

The repair removes repeated immutable-observation audit and passage-text allocation within each transaction. Complete citation membership is still independently streamed in exact order, partial prefixes retain point checks, and post-effect grant/digest/scope/intents/history/artifact audits remain. Public readers use one deferred transaction. An independent read-only review found no material lost invariant. A regression swaps two valid citations in a rehashed complete snapshot, requires refusal, then restores the valid payload and checks successful read and busy-timeout cleanup. The guard, VM/lock/byte/citation limits and boundary fixture remain intact.

A focused 64-member debug run after the repair measured admission at 9,720 microseconds, maximum tick at 12,295 microseconds and final read at 5,811 microseconds. The snapshot filter then passed 40 existing tests before the additional ordered-citation regression, which passes in full verification. These are synthetic workstation observations, not a CI pass or supported latency profile. Final local gates are recorded below; hosted outcome belongs to the exact source checkpoint.

## Withdrawal and native recovery

[Decision0079](../../docs/decisions/0079-task-interest-withdrawal.md) preserves old admission-only cancellation and adds explicit versioned owner withdrawal. Valid production cases cover task-only jobs, surviving direct/monitor interests and separate task jobs. Globally unique owned occurrences make two tasks sharing the same job unreachable today; a separately labeled query reference model tests that future ownership case without claiming valid catalog reachability.

The final withdrawal filter passed 18 tests, including real contained child termination, recognition/translation completion with regressed clocks, insertion failure/rollback/retry and reopen. A family/generation collision test passed with verification, recognition and translation sharing an identifier/generation while unrelated stop flags remained clear. Five earlier actor tests include a real local-control server restart and unresolved hold. Counts overlap and narrower branch receipts retain their source scope.

The real Windows child fixture exercises contained-group lifecycle, durable stop intent, actor reconciliation and actual empty-group completion through the existing capability. The same transaction commits final job state, completion receipt and available worker-cost observations. It is not a speech model, human review or language-quality test. Other storage completion fixtures use explicitly synthetic outcomes.

Review repaired two production defects: cancellation signaling previously selected an identifier/generation without its job family, and clock regression could prevent valid native drain proof from being finalized. New targets bind family, job, generation, attempt, lease owner and task. Effective completion is no earlier than job creation/start or stop intent; the receipt separately preserves the observed clock.

Restart before committed native completion retains cancelling state, generation, leases and scratch; new native claims and global scratch cleanup hold. Capture, read operations and non-native verification remain usable. There is no PID-based invented proof or automatic unknown release. The actor can lose its in-memory completion capability on a catalog commit failure; that case conservatively remains held. Borrowed-capability rollback/retry fixtures do not claim an actor retry-state implementation. Legacy untracked recovery and broader manager/creation-time containment remain qualification gaps.

## Country reference and terminal experience

[Decision0080](../../docs/decisions/0080-offline-country-reference.md) and the [asset provenance](../../assets/countries/cldr-48.2.0/README.md) define 257 territories, pinned CLDR 48.2.0 and Unicode 17 folding, and exact display locales `en`, `ar`, `es`, `fr`, `hi`, `pt`, `sw` and `zh`. Regional/unlisted locales report English fallback. Alias matching preserves original scripts and diacritics under canonical normalization/folding; it does not supply transliteration or locale collation. Ambiguity considers the full reference before paging. Country cursors bind query, locale and reference identity; station-ID paging remains unchanged.

Root inspection covered production test-backend renders at 20x8 and 80x24, plus Arabic search at 132x40. The compact footer retains choice count, locale and page availability; long edited queries expose their end. A stale station response cannot apply an earlier country draft over a newer query.

The actual Windows console driver ran six cases: empty/16-station cache at 20x8, 80x24 and 132x40, with exact dimensions verified by console APIs. It retained 48 real cell/attribute frames. All cases showed 257 choices, initial `AC`, paging to `AZ`, explicit `CD` draft selection and second-Enter applied scope. Four complete read responses, for radio status, recordings, schedules and doctor, were byte-identical before/after within each library. Cache size, capture, policy, budget and charges stayed unchanged; clients exited zero and owned helper processes were absent after cleanup.

Windows refused native pixel capture. No native PNG, font/glyph shaping or key-to-frame/SSH qualification follows. Rasterized test-backend views remain separate evidence. The console driver's later Unicode/Tab sequence occurred in the filter editor and does not establish picker locale cycling. Actual tested executable SHA-256 is `f95818eaad6b8880f4e1c454c0d55c4d5839ce60fd7782101f71e4536a5d838e`; a subsequent CLI dispatch-only refactor was covered by full verification, while country/explorer production modules stayed unchanged.

## Integrated receipts and remaining gates

| Gate | Current outcome |
| --- | --- |
| `cargo verify` | Passed after repair: 922 ordinary test executions, 17 ignored, formatting, warnings-denied Clippy, build, native-source hashes and advisory audit of 313 dependencies against 1,290 advisories |
| `cargo verify-media` | Passed after repair: all 16 fixtures, 105.20 seconds of native test execution |
| `cargo verify-coverage` | Passed after repair: sigy 88.28%, sigy-core 94.41%, sigy-service 93.10%, sigy-test-recognizer 90.81%, sigy-xtask 93.00%; no source exclusions |
| Documentation | Passed: 24 changed/new Markdown files, 796 local links/anchors, complete C49/R68/D43/E33 registers, 27 package definitions, writing rules, README legal/setup preservation and `git diff --check` |
| Exact-commit CI | Initial run failed as described above; a passing repair commit is required for source-only publication, with its outcome retained in checkpoint metadata |

Private logs remain in ignored `.agents/`. Current repair SHA-256 receipts are:

- `milestone-repair-verify-20261003.log`: `6bcb7418b217cc39b912ccd71e807858353797fca9b0d5103c073048db31a032`.
- `milestone-repair-media-20261003.log`: `bf261f27f12f71011c27da0417e61234b746eeae41525950d3d93087dfff0105`.
- `milestone-repair-coverage-20261003.log`: `fe0f324eda110983679576c3a9f96812af2d2ea24000605711fbc87c2faae9cf`.
- Coverage JSON `target/coverage-reports/workspace-13848-1791049260152704400.json`: `99d1c0881d13221a2f95f57974e6711aac263f22fb8a5c29af801f7687fae57c`.
- The 388-file repair runtime/manifests source map: `f6cc0111a8c35ce33b23b5b4c4e8af7f6f290c40ea021233b233121fd5f690c5`. Runtime/manifests and all 14 asset/legal hashes stayed unchanged throughout these repair gates.

The following initial-gate receipts retain their historical source scope:

- `milestone-verify-final-20261003.log`: `0b477d7381f10b0abbc71a604fe0094ab5e50f15caab8b2d82eb59b105dfe492`.
- `milestone-media-20261003.log`: `cb918667a33c728467c56427e0e0673fa4680919bac4cb462ad7ed7726bf75ad`.
- `milestone-coverage-20261003.log`: `0df7110fd482a1baad73f90dcf2f36507f6d41d3a1e8c0d2f4feccb3a55ea581`.
- Coverage JSON `target/coverage-reports/workspace-3156-1791045644594465500.json`: `36a7e73621c1987a48ad75436d285d87576dd4f733da7ab5d42a587d52331fcf`.
- The 387-file runtime/manifests source map: `d61fc0729b570ab28b252541cf22cc966bed6192b51cbd85e37290c2fa2b8472`.
- The post-commit 387-file source map: `c1bcef9fa65d5f049f6fe4280554c58626282e301912c590870e8367fc025e08`. Git normalized mixed/CRLF line endings to LF in control task dispatch, snapshot audit and its recovery fixture; the other 384 source hashes match. Fresh hosted verification checks the committed checkout.
- The 14-file asset/legal source map: `85ab4f626ef43cc35c995cf0806104f63258f6e2a7f7534d40a1ac6bdaf99b87`.
- Native console receipt `country-native-20261003/20261003T162956Z/validation-receipt.json`: `235ab1f7d5307646c2011cc5db6f676a38b501ca6ab3d8bb0e7ca0a4cf469f45`.

These receipts preserve exact local evidence; historical logs and synthetic fixtures do not establish supported operation. Remaining work includes station name ordering, native glyph/latency inspection, city discovery, retained multi-segment/cue playback, native profile qualification, aggregate host admission, target-aware migration, independently measured language directions, bounded local planning and long-running/power-loss/clean-host recovery. Later unusual-claim and SETI research adds no detector or archive import to this milestone.
