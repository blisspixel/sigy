# Terminal finding navigation

Date: 2026-09-30. Status: implemented, tested and rendered buffers inspected on Windows x86_64. This is named citation inspection and navigation to original recording metadata. Finding enumeration and cue playback remain open.

## Decision

Press `5` for Findings and `/` to edit `MONITOR FINDING`. Enter reads that named stored citation through the existing `ShowFinding` operation. Each ID is bounded to 128 ASCII bytes, and the editor is bounded to 257 bytes. A narrow viewport shows the query tail with an omission marker where space permits, preserving the full lookup identity. Arrows scroll the citation; `r` repeats the named read. Typing `q` in the editor inserts a character; Escape leaves editing and `q` then detaches the client. Compact help follows those controls, and the tiny recovery view shows `^C quits`, which still works during editing. Navigation does not publish a finding, enable classification, admit processing or grant capture authority.

The view preserves the stored transcript and translation revisions, original script, uncertain English or untranslated reason, stale indicators, recording identity and half-open cited interval. Original-script and English display are each limited to 4,096 characters, with that limit stated. Text passes through the existing terminal sanitizer and wraps by terminal cell width without splitting graphemes. Scrolling reaches the wrapped text, including on compact layouts. The stored evidence is unchanged.

At the supported minimum of 20 by 8 cells, the finding body keeps one row for citation detail by omitting its repeated instruction line. Errors join the scrollable detail instead of permanently consuming that row. Linear mode budgets its focus marker before rendering the query tail. Both compact and full help distinguish text editing from quit controls.

The original state is explicitly the statement at publication. It does not assert that the media remains available. Press `o` to read the cited recording through `RecordingOperation::Show` and select its metadata timeline in Recordings. A catalog availability statement requires the whole cited interval to fit in one unreleased published segment of a reserved or retained recording, without an overlapping gap. An expired or missing citation has no stored interval and cannot acquire one from this read. The file is not opened or hash-verified; these are separate metadata snapshots, without retention protection or a playback guarantee.

A failed lookup keeps the previous citation. A mismatched monitor, finding or recording response cannot replace it or select another original. Recording navigation keeps the full recording ID and station selection stays separate. No schema, IPC version, source authority, provider route or service operation is added.

## Verification

Focused tests cover bounded and hostile editor text, explicit reads, malformed identity pairs, mismatched response preservation, stale revisions, original-script display, untranslated output, retained and released segments, whole-recording deletion, split-segment refusal, overlapping gaps, missing intervals and grapheme wrapping. A real empty catalog fixture checks that failed named and recording reads preserve the displayed citation and admit no capture. Render fixtures cover 80 by 24, 132 by 40, 40 by 10 and 20 by 8 layouts, including linear mode, editing and errors. They preserve actual buffer receipts under ignored `.agents/` when requested. Integrated outcomes belong in [active work](../development/progress.md).
