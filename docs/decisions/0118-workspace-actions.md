# 0118: Workspace actions

Date: 2026-10-24

Status: implemented one action table per explorer workspace. The footer and the `?` overlay read that table. `s` toggles Globe spin. `r` reloads in every workspace, including Globe. The hint still ends `? more`. Catalog schema stays v54. Local IPC stays v58. Issues 38 and 40 through 49 stay open. No stage exit is claimed.

## Decision

One action table per workspace lists the key, a short footer label, a group (`Move`, `Find`, `Listen`, `Keep`, or `View`), when the footer shows that pair, and an optional command. The footer keeps pairs from the front until the width is full. It still ends `? more`, which is the interface-language contract. The proposed suffix `? keys` is not used. The overlay lists every action, grouped. When slow turning is unavailable, the Globe footer hides `s spin` and the overlay still names that key with the reason `Globe turning is off in this presentation mode.`

`s` toggles slow turning on the Globe canvas. `r` reloads local state in every workspace, including Globe. Capital `S` still stops a listen. Capital `H`, `L`, and `K` stay pause, latest sealed interval, and one hold. Dispatch match arms stay as they are, except the Globe spin key.

Each workspace names Home as its parent screen. Home has no parent. A test checks that mapping. The running client does not read it yet: this slice does not draw Home, does not add a Back key, and does not open a home menu.

Fetch commands on the overlay come from `sigy_service::guidance`. Play, stop, retained listen, and hold use the existing command shapes. The table does not invent a second stop command.

Schema stays v54. No migration. Local IPC stays v58.

## What this does not do

This slice does not implement the home menu, a breadcrumb, or a shared Back key. It does not change playback admission, recording publication, or HTTP acquisition. It does not add a claim mutation or run `claim candidates` from the explorer. It does not fetch news, scrape a page, or set a score. It does not enable Play or Stop on the desktop desk. It does not wire metadata playhead controls to the decoder.

## Interface

The Live footer shows `P play` and `S stop`. The Globe footer shows `P play` and `Enter inspect` at 80 columns, and it does not say `r spin`. At 20 columns the first pair stays short enough that `? more` still fits. Linear and monochrome help text and overlay text use the same words. Editing still shows `Esc ends edit` and `Enter applies`, and `g first page` only when a catalog restart is waiting on Explore or Globe.

## Evidence

`cargo verify` exited 0 in 297.03 seconds. The `sigy` binary tests passed 409, with 7 ignored, in 24.02 seconds. The `sigy-service` library tests passed 774, with 2 ignored, in 172.83 seconds. `cargo audit --deny warnings` loaded 1352 advisories and scanned 382 dependencies. This slice does not change playback admission, recording publication, or HTTP acquisition, so `cargo verify-media` was not run. Coverage was not run. `cargo visual-qa` exited 0 in 11.16 seconds and wrote 78 synthetic frames. The Globe turn frame shows `s spin` and `r reload`, and it does not show `r spin`. Live at 80 columns shows play and stop. The agreement test checks that linear and monochrome hint and overlay words match. External spend is USD 0. This receipt was written after those commands. Hosted verify of this commit is not recorded here.

## Limits

Issue 38 stays open. Issues 40 through 49 stay open. Home is recorded for a later menu and is not drawn. Native key-to-frame timing remains unmeasured. The gallery is synthetic offline frames and is not native terminal fidelity. No acoustic, Omarchy, language, or platform qualification is claimed. No stage exit is claimed.
