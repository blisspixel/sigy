# Monitor processing

Date: 2026-09-25. Status: implemented and tested on Windows x86_64 with storage and actor fixtures. This is the processing part of roadmap operation 31 and increment 3 ("first stage controller") of the [scaling architecture](../design/scaling-architecture.md#stage-graph). Catalog and IPC v33.

## Decision

The service processes a monitor's new recordings on its own, within the user's caps, through a level-triggered stage controller.

- **Plan.** A pure planner (`monitor::pipeline::plan`) takes facts read from the catalog and returns at most four steps per monitor per pass. It is tested without a runtime. A paused monitor takes no steps. Without a recognition profile in the latest version, nothing is recognized.
- **Candidates.** Completed, retained recordings of the sources the monitor follows now, whose capture started after the monitor was created and that have no recognition step for this monitor, oldest first.
- **Caps.** Queuing recognition charges the recording's decoded audio to the monitor on the current UTC day and to its lifetime total. Recognition is strictly oldest first: when the oldest waiting recording does not fit the remaining daily or total cap, nothing newer jumps ahead, and the rest waits for the next day or a new version. A recording longer than the daily cap is skipped once with `longer-than-daily-cap` instead of blocking forever. The catalog rechecks both caps and the current policy version in triggers, so a planner bug cannot overspend. The caps of the latest version apply to the lifetime total recorded under every version.
- **Steps.** A pass performs each step through the same paths as `analysis admit`, `analysis publish`, `analysis transcribe` and `analysis translate`, then appends an immutable step row: queued with its pin and job, or skipped with a reason. A full queue is temporary: the pass stops for that monitor and records nothing. Any other refusal is recorded once, so it is not retried every pass. A catalog fault stops the service, as other reconcilers do.
- **Shared work.** The pin ID is derived from the recording and the job IDs from the recording or transcript revision and the profile hash, so two monitors that follow one station with one profile share one pin, one recognition and one translation. Each monitor still charges its own caps.
- **Translation.** After a queued recognition ends, a translation is queued for a transcript with text when the version names a translation profile; otherwise the translation step is skipped with `no-text`, `no-translation-profile`, `no-transcript` or `recognition-<state>`.
- **Passes** run at most every 5 seconds from the service's schedule tick. Nothing runs while no service is running.
- **Pins.** The former lifetime limit of 256 analysis pins would have stopped one monitored station within days. The limit now applies to pins admitted but not yet published; published pins are bounded by retained recordings, which retention and quota bound.

`monitor show` reports audio processed today (UTC) and in total, queued steps, and skipped steps by reason.

## Evidence

Planner tests cover oldest-first order with no queue jumping, the daily cap, the total cap across days, the skip of an over-long recording, the per-pass bound, pause, translation only after recognized text, and stable, bounded, shared IDs. Storage tests cover facts, charged usage by UTC day, recognized-job facts, exact step replay and a conflicting replay, immutability, a catalog refusal of a step over the daily cap, a refusal from a stale policy version, and the open-pin bound. An actor test runs real passes: no steps without a profile, one shared recognition job for two monitors, a repeated pass that changes nothing, and a pause that holds.

Real run, 2026-09-25, Windows debug build, fresh temporary library: two monitors followed Radio Exterior de España with the pinned `turbo-q5-cpu` recognizer (whisper.cpp b5130, large-v3-turbo q5_0, Silero VAD, 4 threads); one also named the `hy-mt2-cpu` translator. A 20-second `record start` published 23.3 seconds. With no further command, the service pinned the recording, queued one recognition job shared by both monitors (87.7 s wall), charged 23.3 s to each monitor, queued translation for the first monitor (2 of 2 cues translated, 43.0 s wall), and recorded `no-translation-profile` for the second. The station was carrying an Arabic program about Ibn Battuta at that moment; after the first monitor was revised with `ar:ابن بطوطة` and `en:Ibn Battuta`, `monitor matches` cited both cues in the Arabic original and in the English translation. One short capture is operational evidence, not a quality or capacity claim.

## Limitations

- Recordings longer than 60 seconds are refused by recognition until chunked recognition exists; each is recorded once as skipped with `recognition-input-limit`, so it is not retried later.
- There is no fairness between monitors beyond the per-pass bound; operation 28 adds classes, deadlines and capacity.
- The daily cap counts queued audio, not audio actually recognized; a failed job still counts.
- Days are UTC, not the monitor's or the user's time zone.
