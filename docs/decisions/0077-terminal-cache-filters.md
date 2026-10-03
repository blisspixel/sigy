# Terminal cache filters

Date: 2026-10-03. Status: implemented and locally tested on Windows x86_64; verification evidence belongs in [active work](../development/progress.md). Catalog schema stays v44 and local IPC stays v45. This is a partial EX-01 increment in the [engineering plan](../development/reliability-and-scale.md#explorer-increments), following [terminal usability](0076-terminal-usability.md). It establishes no roadmap stage exit.

## Decision

In Explore and Globe, `F` opens an explicit filter editor for station name, country code, directory language, directory tag, upstream check success and favorites. Wide layouts show every field and the applied query; compact layouts show the selected field, read/validation state and applied scope. `Tab`/`Shift-Tab` or up/down select fields, `Space` toggles boolean fields, and `Ctrl-U` clears the selected field. `/` keeps the focused name editor. Backspace removes a whole grapheme; each text field remains within the service's 128-byte bound and uses the existing terminal sanitizer.

`Enter` explicitly submits one existing cached `radio search` operation with at most 16 rows. A matching successful response commits the applied scope with its rows and paging state. Editing changes only the draft. Validation, failed reads and disconnects preserve the last applied rows. Failed and stale responses cannot leave pending page state or replace a newer query. `Esc` discards the current draft and returns to results; an already submitted read retains its authority. `x` from results resets every filter and requests page 1. Reload and paging use the applied scope, including while another draft is open. A new applied scope resets paging; selection survives by station identity when present.

Country currently accepts a blank value or two ASCII letters, normalized to uppercase. It validates syntax, not membership in a worldwide reference. Country names, native-script aliases and cities are not resolved. Language and tag match whole directory labels under the existing service comparison. Declared language does not establish observed speech, and upstream health does not establish local playability. Results describe the current cache. Opening or applying filters does not refresh the directory, contact a station, play audio, capture, send a directory click or grant processing authority.

## Evidence and remaining work

State, client and rendered-frame checks cover combined filters, real local-catalog reads, service validation failures, stale success/failure, cancellation, reload/paging, invalid and disconnected drafts, Unicode bounds and unchanged capture/playback state. Production rendering is inspected at 20x8, 59x17, 80x24 and 132x40, including compact failure states. The final gate receipts and limitations are recorded in active work.

Identifier ordering and the existing station-ID cursor remain unchanged. A worldwide country selector needs a selected, licensed, pinned reference independent of station inventory. Name ordering needs a deliberate query/order/generation cursor and index design. Major-city resolution, asynchronous responsiveness under delayed service replies, sustained terminal performance, assistive-terminal qualification and the integrated DVR remain separate work. No global inventory or operational capacity is claimed by this editor.
