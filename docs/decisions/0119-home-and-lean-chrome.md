# 0119: Home and lean chrome

Date: 2026-11-03

Status: implemented. `sigy tui` opens on Home unless a startup filter is set. A breadcrumb replaces the tab row. Catalog schema stays v54. Local IPC stays v58. Issues 38, 40, and 42 through 49 stay open. No stage exit is claimed.

## Decision

A blank station filter opens Home. A name, country, language, tag, or upstream-health filter opens the workspace that filter already selected. `Explorer::new` still starts on Find stations, so a constructed explorer keeps that surface until `show_home`. The production entry checks the filter before the first search moves it.

Home lists seven tasks in number-key order:

1. Find stations
2. Listen
3. Recordings
4. Watch topics
5. Findings
6. System
7. Globe

Enter opens the selected task. Digits `1` through `7` open that task from Home and from a workspace. `e` leaves Home for Globe and requests one random cached station. `0` returns Home from a workspace, a monitor detail, a claim page, and station context. On Home, `0`, Esc, and Backspace change nothing.

Esc and Backspace go up one level. A confirmation, the key overlay, and text editing each consume one Esc. Station context returns to the same workspace. A claim page returns to the claim list, and the list returns to Findings. A monitor detail returns to the monitor list. The next Esc from a feature screen returns Home. Home is the parent of every workspace, so Esc from Globe returns Home.

The breadcrumb replaces the tab row. At 60 columns and at least 16 rows it reads `Home`, or `Home /` plus the task name. Below 60 columns or below 16 rows there is no breadcrumb row. The connection line names the surface instead: `Sigy Home` or `Sigy` plus the task, then the service state. Digits `1` through `7` and `?` still reach every workspace. Left and Right still cycle workspaces when a workspace is open. The hint is one line and still ends `? more`.

Cache and search rows stay on Find stations and Globe. Identity stays on Find stations, Globe, Recordings, Watch topics, and Findings. At 80 by 24 with a listen or recording active, the rows before the body are the connection, the breadcrumb, the live row, and the hint. A retained listen adds its own row.

The shared live strip says `Listen:` plus the state, and the station name when that listen was confirmed. It omits the listen session id. The Listen workspace body still shows `Listen session:` and that receipt. A recording on the strip still shows its id. Browsing stays independent of the listen.

Home's hint pairs are `Enter open`, `e explore`, `r reload`, and `q quit`. On a workspace, `Esc back` is the second pair, so 20 columns keep the workspace's first pair. The key overlay title is `Keys: Home` or `Keys:` plus the task name. `P` and `S` do not start or stop playback from Home. `H`, `[`, and `]` still reach an already running retained listen, including while Home is open.

Each workspace keeps the action table from [Decision 0118](0118-workspace-actions.md). Home has its own table. A key that changes Home is in that table. `0` and Esc are in the shared table because they change a workspace, and they are absent from Home's table because they change nothing there.

Schema stays v54. No migration. Local IPC stays v58.

## What this does not do

One top line that combines connection, live state, and cache remains later. So do the hint on the last row, a one-line globe header, a one-line station identity, and a globe canvas of at least 15 rows at 80 by 24. The station name stays out of the breadcrumb. A back stack that returns Globe to the previous station list remains later. Esc from Globe returns Home.

The `:` palette remains later. Jobs stay off Home until that screen exists. The Space action menu, confirmations that name their target, one Listen screen for now playing and favorites, and one recordings row per recording remain later.

This slice does not change playback admission, recording publication, or HTTP acquisition. It does not change the catalog or spending. It does not close issues 38, 40, or 45.

## Interface

Home shows the seven tasks, a short summary on each row, and a reversed selection. Find stations names the selected station, or says that no stations are cached. Listen names the state and station, or says that no listen is loaded. Recordings names the loaded count and the active captures. Watch topics says `Not loaded` until the list has been read. Findings names the current query, or says that lookup is by name. System says service, storage, and budgets. Globe says directory map. The menu does not show Jobs or `:`.

A workspace shows `Home /` plus its task. Find stations and Globe keep Cache and Search. Listen, System, and Home omit those rows and omit the selected-station identity. The Listen body still prints the session receipt, including an id such as `listen-1`. The default strip omits that id.

## Evidence

`cargo verify` exited 1 in 251.47 seconds on this Linux host. Formatting and warnings-denied Clippy passed. The `sigy` binary tests passed 388, with 5 ignored, in 14.81 seconds. The `sigy` integration tests, and the `sigy-core` library tests (39), passed. The `sigy-service` library tests passed 762, with 3 ignored, and 13 failed in 131.89 seconds. Each failure is native admission while this process is outside the delegated `sigy-control` cgroup. The shell cgroup is a Ghostty app scope under `user.slice`, and no `sigy-*.service` user unit is loaded. The Windows hosted `cargo verify` job runs this library suite with Job Object admission. This slice does not change playback admission, recording publication, or HTTP acquisition, so `cargo verify-media` was not run. The workspace build step did not run, because verify stops on the first failed check. Coverage was not run. After that stop, `cargo audit --deny warnings` loaded 1403 advisories, scanned 382 dependencies, and exited 0. `cargo visual-qa` exited 0 in 3.87 seconds and wrote 84 synthetic frames, including Home and Home offline. `cargo readme-hero` exited 0 in 0.35 seconds and rewrote `docs/images/hero-dark.png` and `docs/images/hero-light.png`. The hero strip says `Listen:` and `playing` and names North Coast. The breadcrumb says `Home / Globe`. The frame has no `play-1842`. External spend is USD 0. This receipt was written after those commands. Hosted verify of this commit is not recorded here.

## Limits

Issues 38, 40, and 45 stay open. Their remaining checklist items are still open, including a Globe path back to the previous station list. Issues 42 through 44 and 46 through 49 stay open. Native key-to-frame timing remains unmeasured. The gallery is synthetic offline frames and is not native terminal fidelity. The Linux library failures above are this host's missing delegated unit. The explorer tests passed on this host. No acoustic, Omarchy, language, or platform qualification is claimed. No stage exit is claimed.
