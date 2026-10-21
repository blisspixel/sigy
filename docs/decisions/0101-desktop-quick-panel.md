# 0101: Desktop quick panel

Date: 2026-10-21

Status: the read-only keyboard desk is implemented. Confirmed Play and exact Stop from that desk wait until one owner process is proven across dismissal, unload, and shell exit. Qualification flags stay false. This is not an Omarchy, audio, or stage qualification. Schema v51 and local IPC v53 are unchanged.

## Decision

Issue 36 adds `sigy panel desk` beside the existing [panel commands](0088-omarchy-desktop-panel.md). The desk is a keyboard surface in the existing `sigy` binary. It reads the same projection as `sigy panel status`: service presence, directory freshness, at most four cached favorites, a partial-page marker, active capture count, and the current listen receipt. Opening, moving, reloading, and quitting perform no stream contact, directory refresh, click, capture, recording admission, or paid processing. `r` repeats that read. It does not fetch the directory.

Selection is local. Enter on a favorite or on More in Sigy does not play. Enter on Open full Sigy restores the terminal and prints `sigy tui`, including `--data-dir` when the user passed one. The desk does not spawn that process and does not hand off a selected station. Waybar's sample click stays `sigy tui`. A user may point a launcher at `sigy panel desk`; Sigy does not edit desktop configuration.

The listen receipt carries a source revision and no station id. The desk and `panel status` show `Station: unknown` unless a later read fills `station` from the catalog. `panel-session.json` remains a hint. Its station id is not copied into the projection. `source_revision` is added to the listen object. `station` is JSON `null` until that catalog read exists. Older readers that ignore unknown fields keep the previous object.

Listen words are the receipt state, including `starting`, `running`, `stopping`, `failed`, and `interrupted` when the receipt uses them. The catalog's own states remain `running`, `completed`, `failed`, and `interrupted`. Capture stays a separate count. Stopping a listen is not offered here, so a capture cannot be stopped by this surface. `sigy panel play` and `sigy panel stop` keep their current admission and lifecycle. They are not bound to the desk.

A pipe, a redirect, or `TERM=dumb` prints one plain line and `Next: sigy panel status`, then exits 1, before library contact and before the terminal session. Normal quit exits 0. An external stop restores the terminal and exits 130. The desk uses the same alternate-screen and synchronized-output modes as the explorer. Reduced motion, linear mode, and monochrome mode are visible in the frame. The desk does not animate in any mode. Color uses the existing terminal role slots. Selection uses a `>` marker, and a reversed bar when linear mode is off, so it does not depend on color.

Packaging adds no runtime, plugin, or product language. Ordinary `sigy` commands gain no desktop dependency. Installation and removal of a desktop launcher stay explicit, per user, and reversible. There is no marketplace publication.

The closed interface issues 24 through 34 already cover explorer hierarchy, copy, help, semantic color, globe polish, and non-blocking metadata reads. This increment does not reopen them. Three gaps stay outside the status-only slice: a read-only revision-to-station lookup without a second catalog connection, selected-station handoff into another client, and an owner process for Play and Stop that survives hiding the desk.

## Evidence

Local `cargo verify` passed on this Windows host: formatting, warnings-denied Clippy, workspace tests, the build, and `cargo audit --deny warnings`. The `sigy` binary tests passed 355 with 7 ignored. The `sigy-service` library tests passed 734 with 2 ignored. Focused panel tests passed 11 binary tests and 3 integration tests. Coverage was not re-run. `cargo verify-media` was not required. The progress receipt was written after that run. No acoustic, Omarchy, or platform qualification is claimed. External spend remains USD 0.
