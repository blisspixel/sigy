# 0082: Frozen station context in the terminal

Date: 2026-10-03. Status: implemented; verification evidence is recorded in [active work](../development/progress.md). No release support or stage exit is established.

## Decision

Enter in Explore or Globe opens focused inspection of the selected station. Retain the full metadata already received in the bounded ordered station page alongside its clipped list preview. Opening and closing inspection are local state transitions with no service operation, directory refresh, station contact, registration, playback or capture admission. This avoids adding a second read whose catalog consistency and allocation bounds differ from ordered paging.

The pane freezes one exact station UUID, its metadata, displayed catalog identity and library capture/play-session snapshot. A metadata identity mismatch falls back to the preview rather than mixing stations. Pending search or favorite requests must finish before opening. Existing request-generation and catalog fences continue to reject stale replies. Disconnect retains the observation; reported catalog drift stays visible and requires Back followed by explicit `g` restart. Inspection is not an automatic health check.

Escape or Backspace reveals the same applied scope, draft, page cursor/history, selection, workspace and focus. Up/Down scroll the context; other browsing and mutation keys have no effect while it is open. Quit detaches without stopping background work. Full refresh responses cannot replace the browser under an open pane. There is no asynchronous-query or latency guarantee added by this local composition.

## Presentation and limits

Compact terminals use a focused column with an unshortened wrapped UUID, visible Back/Quit and scroll instructions. Nonlinear views at least 112 columns and 20 rows keep the cached results beside the context. Linear mode keeps one column. Grapheme-based wrapping preserves combining sequences and wide characters; display controls and directional overrides remain filtered. Unknown labels remain explicit, including legal empty directory labels.

The full fields preserve the existing station and ordered-page bounds: 256-byte name, 32 labels per list, 128-byte labels, 2,048-byte origin, 8 KiB stored metadata before copying, at most 16 decoded rows and the capped page/frame. The client retains those already bounded fields and clones one selected observation. No additional database query, cursor, scheduler, job or dependency is introduced. Schema stays v48 and local IPC v49.

Ordered reads omit global directory statistics. They preserve the prior observed statistics within the same catalog namespace/comparison rather than fabricate zeros. A namespace replacement invalidates them and displays unobserved statistics until an explicit reload. Those counters remain a prior snapshot, not a new measurement of every displayed catalog revision.

Directory languages, checks, coordinates and codecs remain dated listing claims. Registered revisions and recordings are explicitly unresolved in this station view: existing global lists cannot establish a station relationship, and names/URLs cannot replace immutable directory links. Library capture counts and play-session metadata are labeled separately; a session state does not prove audible playback. Actual protected retained playback follows DV-01A/B.

## Verification

Acceptance checks exact second-page return with different applied/draft countries, modal zero-effect keys, pending and stale responses, identity mismatch, disconnect/drift, unknown labels, control injection, full names, combining/native-script boundaries and scrolling through resize. Render and real-console evidence must distinguish test fixtures from actual terminal cells, and neither establishes font shaping or broad platform support. [Near-term implementation](../development/near-term-implementation.md#ex-02a-focused-listening-context) and active work hold the current next increment and measured results.
