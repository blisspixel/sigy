# 0094: Random cached station exploration

Date: 2026-10-13

Status: implemented bounded discovery increment. Active work records verification.

## Decision

`sigy explore` and `e` in the Explore or Globe workspace choose uniformly from cached stations with valid latitude and longitude. Name, country, directory language, tag, directory health and favorites use the existing applied filter semantics. The eligible set includes the entire partial cache, beyond the displayed page. This is discovery within collected directory observations, not a uniform sample of worldwide broadcasters.

The service counts the full eligible set, draws an unbiased rank with rejection sampling from the existing operating-system entropy dependency, then reads that UUID-ordered rank. Count, selected metadata, favorite and catalog identity share one deferred snapshot and the existing 100 ms cooperative guard, four-million SQL VM operation ceiling and 10 ms lock wait. At most 16 entropy draws are attempted; failure returns an error rather than a biased fallback. The selected row passes the existing bounded metadata, endpoint-origin and name-key checks. The response encoding is capped at 16 KiB. Exhausted work refuses the request; partial scan sampling is forbidden.

Schema stays v51. Local IPC advances to v53 for `Explore` and a distinct `random_station` result containing optional station, favorite flag, eligible count and catalog identity. It has no page cursor. Ordinary search and its historical pagination contract remain separate. No random seed is accepted from public clients; deterministic tests inject an internal entropy source.

The terminal fences replies by request generation and catalog identity. A successful selection switches to Globe and cuts directly to the station, including under reduced motion. It stops idle rotation. The view labels a random pick and the count of eligible cached stations; `e` repeats and `g` returns to ordered results. Empty eligibility names the missing coordinate matches and offers explicit refresh. Directory positions are labeled approximate; they do not locate a speaker or subject. Monochrome uses the existing rendering path and selection marker.

Selection performs no directory refresh, stream probe, click, source registration, playback, capture or analysis. Existing confirmed play remains explicit. The CLI exposes the same service read, including structured JSON, without opening the TUI. Metadata reads still use the existing synchronous client path; no new latency or platform qualification is claimed.

## Evidence

Fixtures enumerate every eligible rank across 38 coordinate-bearing rows, including rows beyond page 1; cover all filters, favorites, empty eligibility, corrupt selected metadata, rejection/exhaustion, stale request/catalog replies and invalid coordinates; and exercise both motion modes. A service-backed CLI fixture counts directory and stream connections before and after repeated picks, then reads offline. The production gallery includes selected, empty and monochrome random views. Exact integrated gates and receipt paths belong in [active work](../development/progress.md).
