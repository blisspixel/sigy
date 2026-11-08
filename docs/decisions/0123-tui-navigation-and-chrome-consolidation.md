# 0123: TUI navigation and chrome consolidation

Date: 2026-11-08

Status: implemented. Lean chrome, consolidated top bar, bottom keyboard hint bar, dynamic breadcrumbs, compact station identity, and collapsible drawer/side panel on globe with full zoom toggle (`z`) and direct selection glide are in place. Catalog schema stays v54. Local IPC stays v58. Issues 40, 41, and 45 acceptance checklists are met. Issues 13, 14, 38, 43, 44, 48, 49, and 53 stay open. No stage exit is claimed.

## Decision

The top chrome consolidates the connection state and workspace breadcrumb into a single, compact top bar when terminal width permits (>= 80 columns), reducing wasted rows while keeping all status tokens whole. Breadcrumbs reflect hierarchy (`Home / Find stations / {Station}`, `Home / Recordings / {ID}`).

The keyboard shortcuts/hint bar moves to the bottom of the screen in full sections, following standard TUI layout conventions. In compact small-terminal layouts (< 16 rows or < 60 columns), existing compact layout constraints are preserved.

Station detail identity blocks are compressed into aligned lines, displaying station name, directory health, observed age, HLS mark, and directory languages cleanly, allowing up to 15-16 visible station rows at standard 80x24 (up from 13 rows).

On the Globe workspace:
1. The header is compressed to 1 line, retaining view projection, center coordinates, geometric night UTC instant, and approximate mapped/total station counts.
2. The station list renders as an alongside side panel on wide terminals (>= 100 columns) or as a compact 3-row drawer at the bottom on narrower terminals (canvas height >= 18), providing at least 15 rows for the ASCII globe canvas at standard 80x24.
3. A full zoom toggle (`z`) toggles full canvas view (giving 18-19 rows at 80x24). `Esc` unzooms when zoomed.
4. Moving selection through stations on the globe automatically glides or jumps the globe center coordinates toward the selected station.
5. Tab toggles focus between globe rotation (`Focus::Results` / "Tab turn") and station list (`Focus::Detail` / "Tab stations") with visible focus indicators and dynamic hints.

Schema stays v54. Local IPC stays v58.

## What this does not do

This does not change catalog schema, migrations, or database layout. It does not alter media acquisition, playback admission, decoder integration, or spending limits. RF, packet, and IQ contracts are untouched.

## Interface

At 80 by 24, the station list shows 15 rows (up from 13), with the shortcut hint bar at the bottom and 2-line station identity. The Globe view provides 15 canvas rows with the compact station drawer and 18-19 rows when zoomed (`z`).

## Evidence

Warnings-denied Clippy for the workspace exited 0. `sigy` binary unit tests passed all 452 tests with 0 failures (5 ignored). `cargo visual-qa` exited 0 and rendered all 94 frames. External spend is USD 0.

## Limits

Issues 13, 14, 38, 43, 44, 48, 49, and 53 stay open. Issues 40, 41, and 45 checklists are met. Native key-to-frame timing remains unmeasured. The gallery is synthetic offline frames and is not native terminal fidelity. No acoustic, Omarchy, language, or platform qualification is claimed. No stage exit is claimed.
