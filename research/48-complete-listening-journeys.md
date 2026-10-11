# Complete listening journeys and recovery

Reviewed: 2026-10-11. Status: primary-source research and repository review for the next increments. Baseline: `391585e9cc42d3c035c4085375b4f27ce51323a1`, source-only release `v0.1.0-dev.20261011.1`. This note selects a delivery sequence, not new platform, performance or language support. No inference, corpus download, public-stream acquisition or paid service was used.

## Product review

The [README](../README.md) promises discovery, listening, recordings, multilingual inspection and cited findings through one persistent service. [Intent](../INTENT.md) requires an approachable instrument that remains controllable and recoverable. The recent desktop fixes make discovery and quick listening useful; the next quality gain is continuity across that complete journey.

Repository observations, rather than new runtime measurements:

| Source | Current behavior | Consequence for the next increment |
| --- | --- | --- |
| [Terminal loop](../crates/sigy/src/explorer/terminal.rs) and [player](../crates/sigy/src/explorer/player.rs) | Player status and stop use `Runtime::block_on` on the terminal thread; metadata effects are also synchronous | A delayed service response can prevent input/rendering progress. Move player observation/control off this thread first, then extend the same bounded pattern to metadata |
| [Control transport](../crates/sigy-service/src/control/server.rs) and [timeout](../crates/sigy-service/src/control/mod.rs) | Each exchange has a five-second timeout | This bounds an exchange but does not make a synchronous terminal call responsive. Endpoint loading precedes the timed exchange; no whole-call filesystem deadline is established |
| [Directory observer](../crates/sigy/src/explorer/fetch.rs) | Async observation already uses bounded messages and generation fences | Reuse the existing runtime and ownership pattern; no new terminal backend or general event framework is justified |
| [Timeline](../crates/sigy/src/explorer/timeline.rs) and [finding navigation](../crates/sigy/src/explorer/finding.rs) | Timeline/finding views inspect metadata; exact retained excerpts are available through CLI | Add deliberate protected playback actions before presenting a metadata playhead as an audio control |
| [Analysis completion](../crates/sigy-service/src/control/actor/analysis.rs) | Recognition/translation remove the active entry before fallible result settlement; current recovery can retain a hold after losing the in-memory completion capability | Investigate retaining the exact completed outcome until durable settlement, while keeping worker liveness distinct. Never release protection from a timeout or missing PID |
| [Active evidence](../docs/development/progress.md#snapshot) | Windows gates and delegated Linux contracts pass; broad acoustic, mixed-load, language and release qualification remain open | More test count alone does not complete the README journeys |

## Responsive terminal work

[Ratatui's event guidance](https://ratatui.rs/concepts/event-handling/) separates event handling from rendering and leaves event acquisition to the backend. Its examples are architecture references, not a reason to replace Sigy's selected Termina backend.

[Tokio bounded channels](https://docs.rs/tokio/latest/tokio/sync/mpsc/index.html) provide finite capacity and backpressure. The locked 1.53.1 channel source was also inspected locally; rolling documentation currently describes 1.53.2. No dependency upgrade is selected. The versioned channel documentation was inaccessible through the web reader. [Pinned blocking-task documentation](https://docs.rs/tokio/1.53.1/tokio/task/fn.spawn_blocking.html) explains that aborting a started blocking task does not stop it.

Inference: retain the synchronous Termina input/render loop and use existing async IPC with bounded result delivery. Use a separate stop lane so a slow status request cannot queue ahead of stop intent. Poll results without waiting, fence them by request/session identity and preserve mutation IDs across uncertainty. Repeated observations may coalesce; admitted mutations may not disappear or repeat. Moving blocking work to another thread improves responsiveness but does not prove cancellation or bounded filesystem completion.

## Retained audio continuity

[FFmpeg seeking](https://ffmpeg.org/ffmpeg.html#Main-options) distinguishes demuxer seek points from accurate decoded positioning. Its [audio trimming contract](https://ffmpeg.org/ffmpeg-filters.html#atrim) distinguishes timestamp-based bounds from sample counting and leaves timestamp rewriting to a separate filter.

Inference: retain Sigy's existing counted, half-open excerpt contract. Independently generated PCM sentinels should test nonzero origins, resampling, channel identity and exclusive ends. Compressed priming and codec changes need declared tolerances and separate profiles. A progress clock, successful decoder or metadata cursor is not proof of presentation or audibility. Start with one retained reader and no prefetch; next-segment admission follows fresh media validation. A gap, expired segment or open tail must stop visibly instead of silently joining unrelated audio or reconnecting live.

## Recovery and host envelopes

[SQLite WAL documentation](https://www.sqlite.org/wal.html) explains how long read transactions can delay checkpoint progress. [SQLite's backup API](https://www.sqlite.org/backup.html) supports consistent database snapshots; a database snapshot does not by itself include external media or qualify Sigy's restore workflow.

Inference: keep metadata reads finite, measure WAL growth and lock contention during capture, and use the existing owned backup/restore path. Test original-media hashes, immutable history, reservations, unresolved readers and destination preservation independently after restore. Do not introduce a database replacement, NAS catalog or online media backup merely because upstream APIs exist.

For completion commit failure, separate completed native evidence from the active worker map. A bounded retry must bind the exact family, job, generation, attempt and lease owner, audit whether settlement already committed, and preserve cancellation precedence. A process restart still loses uncommitted in-memory evidence; its hold remains until a separately qualified mechanism can prove closure. This is a proposed repair boundary, not a completed recovery claim.

[Microsoft's creation attribute contract](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute) includes assignment of a new process to a list of jobs. [Job Object documentation](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects) describes group limits and lifecycle behavior. Inference: investigate a maintained safe creation-time interface before claiming the Windows suspended-create/assign window closed. The OS facility alone does not provide a qualified Rust integration. First-party unsafe code remains excluded.

## Language and worldwide discovery

[SacreBLEU's reference implementation](https://github.com/mjpost/sacrebleu#confidence-intervals-for-single-system-evaluation) records metric signatures and supports uncertainty estimates and paired comparisons. Inference: preserve the existing Rust scorer, freeze independent reference diversity and normalization, separate recognition errors from translation errors, and report per-variety outcomes. Aggregate scores cannot qualify critical meaning or required Canadian French, Navajo and Klingon cases. The failed judge screen cannot serve as a qualified evaluator. No Python execution or new model selection is proposed.

[GeoNames' extract specification](https://download.geonames.org/export/dump/readme.txt) documents population-based city extracts, capitals, language-tagged alternate names and CC BY 4.0 attribution. Inference: evaluate a pinned offline city/alias extract with a declared inclusion rule, mapping to the country reference and a missingness manifest. A population cutoff is not complete coverage of Sigy's 257 territory identities. Measure import/search cost and preserve licenses before selecting or distributing data. Directory coordinates remain attributed metadata, not reception coverage.

## Delivery recommendation

Followup: [Decision0091](../docs/decisions/0091-responsive-player-observation.md) implements the first bounded player status/stop slice. The source observations above describe the stated baseline; native terminal timing remains unmeasured. The [completion/shutdown audit](49-completion-settlement-and-shutdown.md) narrows the next repair without claiming it implemented.

First deliver responsive player status/stop. Next repair completion-settlement retry without weakening restart holds. Then connect exact retained excerpts to deliberate terminal actions and qualify segment navigation. Measure mixed capture/playback/query/processing envelopes before increasing concurrency. Target-aware history and language references can advance as separate bounded packages; city-data evaluation and source maintenance remain independent research. A local planner should compose capabilities whose authority and outcomes are already inspectable; hardware and additional clients retain their existing later placement.

The [near-term briefs](../docs/development/near-term-implementation.md#next-delivery-sequence) own proposed acceptance and order. Existing roadmap stages, spending limits, local privacy, replay identities and qualification gates remain unchanged.
