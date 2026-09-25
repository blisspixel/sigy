# Implementation progress

Updated: 2026-09-24. Status: active implementation toward the first complete release (roadmap stage 7). No roadmap stage is exited and there is no supported release.

This record holds current state only. The detailed narrative from 2026-09-20 to 2026-09-24, including every earlier verification run, pilot and fault note, is preserved in [September 2026 history](progress-history-2026-09.md). Decision records hold each operation's contract. Research records hold dated external evidence.

## Snapshot

| Item | Current value |
| --- | --- |
| Catalog schema / local IPC | v32 / v32 |
| Verification | `cargo verify`: 432 tests passed, 15 native-media tests ignored, warnings-denied Clippy, build, and `cargo audit` of 312 crates against 1,269 advisories. `cargo verify-media`: 15 of 15 on FFmpeg 9.0.1 |
| Host | Windows 11 x86_64, Ryzen 7 7840U, about 64 GiB RAM, Radeon 780M. Two build jobs, two test threads, one media-test thread |
| Other platforms | Linux x86_64 (Debian 12 container, Rust 1.98.1, WSL2 kernel 6.18): `cargo verify` passes; `cargo verify-media` passes 14 of 15 on FFmpeg 5.1.9, and the native recognizer fixture fails closed with `limits-unavailable` (see open gates). macOS and small always-on hosts are untested. Nothing is qualified |
| Public source | `main` on [blisspixel/sigy](https://github.com/blisspixel/sigy); one [source-only development prerelease](https://github.com/blisspixel/sigy/releases/tag/v0.1.0-dev.20260923) |

## Operations

States: **done** means the operation's local exit evidence exists on this host; **partial** means some of it exists; **open** means not started. Done is not release qualification.

| Op | Work | State | Evidence |
| --- | --- | --- | --- |
| 1 | Play a retained recording | done | [segment playback](../decisions/0027-segment-playback.md), media fixture |
| 2 | Resolve a playlist document | done | [playlist resolution](../decisions/0009-playlist-resolution.md) |
| 3 | Directory click only when asked | done | [directory clicks](../decisions/0010-directory-clicks.md) |
| 4 | Listen to a direct audio revision | done | [direct listen](../decisions/0011-direct-listen.md) |
| 5 | HLS media playlist | done | [HLS media playlists](../decisions/0012-hls-media-playlist.md); [live HLS](../decisions/0042-live-hls.md) added |
| 6 | ICY metadata out of the audio hash | done | [ICY observations](../decisions/0013-icy-observations.md) |
| 7 | Decoded format ladder | done locally | [decoded formats](../decisions/0014-decoded-formats.md); no public station yet |
| 8 | Terminal stack by measurement | done | [terminal stack](../decisions/0015-terminal-stack.md) |
| 9 | List explorer | done | [list explorer](../decisions/0016-list-explorer.md) |
| 10 to 14 | Podcasts: subscribe, RSS refresh, enclosure download, playback, publisher text | done | [0017](../decisions/0017-local-podcast-subscriptions.md), [0018](../decisions/0018-rss-feed-refresh.md), [0019](../decisions/0019-episode-enclosure.md), [0020](../decisions/0020-retained-episode-playback.md), [0022](../decisions/0022-publisher-text.md) |
| 15 to 19 | Intervals, segment seals, gaps, segment playback, holds and pruning | done | [0023](../decisions/0023-recording-intervals.md), [0025](../decisions/0025-segment-seals.md), [0026](../decisions/0026-capture-gaps.md), [0027](../decisions/0027-segment-playback.md), [0028](../decisions/0028-segment-retention.md) |
| 20 | Civil-time schedules | done | [recording schedules](../decisions/0029-recording-schedules.md) |
| 21 | Saved directory refresh policy | done | [directory refresh policy](../decisions/0030-directory-refresh-policy.md) |
| 22 | Analysis pins on published inputs | done | [analysis inputs](../decisions/0031-analysis-inputs.md) |
| 23 | Local transcription | partial | [native recognition worker](../decisions/0039-native-recognition-worker.md) runs in the service with fault fixtures; [three-clip calibration](../../research/experiments/local-asr/three-clip-calibration.md). Per-language benchmark checks remain |
| 24 | Language spans | partial | [language evidence](../decisions/0033-language-evidence.md) storage; recognizer block labels stored as unevaluated evidence. Measured detection remains |
| 25 | English translation aligned to a transcript | partial | [local translation worker](../decisions/0041-local-translation-worker.md): per-cue contained llama.cpp translation with reasons for untranslated cues, fault fixtures. Reference-scored quality checks remain |
| 26 | Provider configuration, dispatch off | done locally | [provider configuration and billing faults](../decisions/0040-provider-configuration-and-billing-faults.md): immutable routes, secret references only, exact price snapshots |
| 27 | Billing faults before any live provider | done locally | [exact provider pricing](../decisions/0037-exact-provider-pricing.md) and [billing faults](../decisions/0040-provider-configuration-and-billing-faults.md): an offline dispatcher proves every listed fault, including a lifetime allowance that never refills. No live transport exists |
| 28 | Live queue over committed segments | partial | [task contract and job pool](../decisions/0043-task-contract-and-job-pool.md): durable queue with leases, attempts, per-kind caps of one and no job history cap. Fair scheduling, host budgets and live segment intake remain |
| 29 | Corrections as appended revisions | open | |
| 30 | Monitor bounds and decision record | done locally | [monitor versions and actions](../decisions/0046-monitor-versions-and-actions.md) |
| 31 | Monitor coverage and scheduling | partial | [monitor coverage and matches](../decisions/0047-monitor-coverage-and-matches.md): per-stage counts with gaps, missed windows and untranslated reasons. Monitor-driven scheduling remains |
| 32 | Passage matches and cited findings | partial | Literal term matches cite recording, transcript revision, cue and media clock. Stored findings remain |
| 33 to 35 | Briefings, projection history, classification off | open | [topic monitoring design](../design/topic-monitoring.md) |
| 36 | Globe and day/night map | partial | [globe geometry](../decisions/0038-globe-geometry.md) and [terminal globe](../decisions/0044-terminal-globe.md): globe and flat map with offline coastlines, geometric night and page stations. Clustering, map and list agreement on one filter, and a measurement with capture running remain |
| 37 | Truthful visualizers | open | |
| 38 | Backup and restore of the release schema | partial | [backup and restore](../decisions/0045-library-backup-and-restore.md): offline backup with hashed media, verification, staged restore; rehearsed on a live-data library at v30. Clean-host and release-schema reruns remain |
| 39 | Power-loss behavior of a segment seal | open | |
| 40 | Routing decision | open | [network routing](../design/network-routing.md) remains a design |
| 41 | Release qualification | open | |

The [roadmap](../../ROADMAP.md#whats-next) explains the order and which work proceeds in parallel.

## Language pipeline

The [language pipeline plan](language-pipeline.md) is the active build goal, including the [2026-09-24 decisions](language-pipeline.md#2026-09-24-decisions): bounded native execution with network isolation recorded as a limitation, a portable CPU baseline with optional acceleration, a required speech-activity gate, lighter acquisition records, and bounded public station audio for validation.

Current evidence: whisper.cpp b5130 with `ggml-large-v3-turbo-q5_0` and the Silero VAD transcribed the three FLEURS calibration clips with loose CER of 1.7% (Arabic), 9.3% (Hindi) and 0% (Spanish), and produced no text for silence. With two capped threads it ran 6 to 16 times slower than real time. Three read-speech clips qualify no language. The [32-clip calibration](../../research/experiments/local-asr/calibration-32.md) (4 read-speech clips per survey language) found turbo loose CER at or below 1.7% for Spanish, French, Portuguese and English, 3.9% for Arabic and 1.3% (letters and digits) for Mandarin, but 24.0% for Hindi and 18.9% for Swahili. Turbo wrote one Hindi clip in Urdu script, and Swahili is the weakest language. All non-speech controls produced no text. Turbo ran about 6 times slower than real time and small about 1.2 times on this CPU. This is calibration, not qualification. Canadian French, Navajo and Klingon remain required cases with no tested model. The [32-clip translation calibration](../../research/experiments/local-mt/calibration-32-translation.md) scored Hy-MT2 1.8B and Gemma 4 E2B on reference text and recognizer output with a sacrebleu-matched chrF++ and BLEU. Gemma scored higher overall (59.7 against 54.7 chrF++ on reference text) and much higher on Swahili, recognizer errors cost 8 to 9 chrF++ points, and both models made critical meaning errors that heuristic flags missed. The set has only four distinct English references, so it ranks nothing; a calibrated critical-error judge is required before any claim. A [Vulkan measurement on the Radeon 780M](../../research/experiments/local-mt/vulkan-780m.md) found llama.cpp 5 to 12 times faster at prompt reading and 1.5 to 5 times faster at generation than the loaded CPU, with no load-time gain. GPU arithmetic changed one Arabic Hy-MT2 translation, so each device is its own profile needing its own calibration. It also found that the translation worker runs on the GPU without saying so when its runtime folder contains a GPU backend; the CPU profile now passes `-dev none -ngl 0`, confirmed on the Vulkan package to keep every layer on the CPU.

New downloads so far total about 25.3 GB of the 100 GB ceiling (see the history file for the itemized policy charges and the [three-clip record](../../research/experiments/local-asr/three-clip-calibration.md) for the two added models).

## Open gates and known limitations

- **Network isolation:** native recognizers are not network-sandboxed by the operating system. Static import checks only. Gate for recognition on user recordings before release.
- **Process containment:** Job Object limits on Windows. On Linux the containment library enforces cgroup limits only at the real cgroup root, so under a systemd session or service, or in a container, native recognition and translation fail closed with `limits-unavailable`; confirmed in a Debian container with and without a private writable cgroup. A delegated-cgroup path (service in one leaf, each worker in a sibling leaf) is required before Linux can run them. macOS fails closed. Suspended-spawn assignment window remains. After a restart, only Windows requeues native jobs; Linux and macOS interrupt them.
- **Recognition scope:** one interval of at most 60 seconds per job; no chunking of longer recordings; model hashing takes seconds per run.
- **Quality:** no language is qualified. The 32-clip calibration and the frozen holdout remain.
- **Durability:** physical power loss, complete media backup and restore on another host, and the 5000 ms segment window as a measured guarantee are unproven.
- **Platforms:** the Linux container run found and fixed a Unix-only Clippy error and an init that left an existing directory at 0755, which made the service refuse to start. Unix parent-death and peer checks, installers and OS startup are unqualified; macOS has not run.
- **Public stations:** the [live station pilot](../../research/experiments/local-asr/live-station-pilot.md) has recorded and transcribed all eight public stations (Spanish, two Arabic, Portuguese, Swahili, Canadian French, Chinese, Hindi). A stream-edge header delay that blocked the Canadian French station is fixed, and that station has since been recorded and transcribed.
- **Live HLS:** [live HLS](../decisions/0042-live-hls.md) recording and master-variant resolution pass loopback fixtures and recorded the three HLS-only pilot stations (Chinese, Hindi, Arabic) after two compatibility fixes; a capture still ends at its first gap.
- **Chunking:** a station recording of more than 60 seconds cannot yet be transcribed; a 60-second request publishes 63 to 74 seconds.
- **Paid processing:** dispatch is unavailable; the product's default paid budget is zero. When enabled, paid use draws down a one-time lifetime allowance that never refills.

## Spending ledger

Cumulative authorized external-spend ceiling: **USD 20**, excluding coding-session costs. Settled spend: **USD 0**. Outstanding commitments: **USD 0**. Available: **USD 20**. The user raised the cumulative ceiling from USD 10 on 2026-09-22 and authorized paid OpenRouter validation within it; this is not an additional USD 20 allowance.

Pushing `main` runs `cargo verify` on one standard GitHub-hosted Windows runner, which [GitHub's billing documentation](https://docs.github.com/en/billing/concepts/product-billing/github-actions) (reviewed 2026-09-22) states is free for public repositories. `cargo verify-media` stays local. No paid provider or recurring service is enabled. Paid experiments require a mechanically bounded allocation and a recorded reservation before dispatch, after the operation 27 billing-fault gate. Every model judge, retry, fallback and unresolved charge counts toward the same ceiling. The application's provider budget remains separately disabled by default.

| Date | Work or allocation | Reserved USD | Settled USD | State |
| --- | --- | --- | --- | --- |
| 2026-09-22 | Language pipeline research and local host inspection | 0 | 0 | Public documentation and local read-only checks; no inference |
| 2026-09-22 | Language-evidence storage, inspection, and local verification | 0 | 0 | Local fixtures and one free parser dependency; no inference or paid request |
| 2026-09-22 | Supervised input verification and public-source preparation | 0 | 0 | Local tests, dependency audit and credential review; no inference or paid request |
| 2026-09-22 | Native process boundary probe and CI maintenance | 0 | 0 | Local bounded child fixtures and action-pin review; no model, paid request, or hosted compute allocation |
| 2026-09-22 | Local network-boundary screen and ASR asset plan | 0 | 0 | One finite trusted-child loopback run and metadata-only model review; no model or paid request |
| 2026-09-22 | Offline language scorer and worker-boundary review | 0 | 0 | Synthetic scorer tests and source-level native worker study; no audio, model, or paid request |
| 2026-09-24 | Three-clip local recognition calibration | 0 | 0 | Two pinned public model downloads and local CPU inference; no paid request |
| 2026-09-24 | Service recognition worker, pricing and globe groundwork | 0 | 0 | Local fixtures and local CPU inference; no paid request |

The plan proposes an initial paid batch of at most USD 2 after billing qualification. It has no reservation yet. Record the concrete request manifest, exact maximum liability, runtime ledger reference, and later settlement here before executing a batch.

## Tracked intermittent test issues

Two historical fixture flakes remain unexplained, with diagnostics added and acceptance conditions unchanged: a directory crash fixture that once timed out waiting for its local request, and a malformed-audio media fixture that once lacked the expected failure detail. Details are in the [history](progress-history-2026-09.md). Investigate with the added diagnostics if either recurs.
