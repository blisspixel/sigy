# Optional terminal globe rotation

Date: 2026-10-13. Status: implemented; verification receipts belong in [active work](../development/progress.md). Catalog v51, IPC v52 and dependencies are unchanged.

The Globe workspace's `r` key toggles optional eastward rotation at six degrees per second. Rotation starts off. Manual `h`, `l`, `j`, `k` or `c` immediately stops it, including a center request without coordinates. Other workspaces keep their existing reload key. Rotation changes only the presentation longitude and never requests service, directory, playback or capture work.

The continuous terminal session explicitly enables motion only with terminal input and output and without reduced motion, linear order or monochrome presentation. The existing environment resolver covers `SIGY_REDUCED_MOTION=1`, `SIGY_LINEAR=1`, `NO_COLOR` and `TERM=dumb`. One-frame inspection and models outside an interactive session stay static. Static modes retain their existing globe header. No third-party code or art is added.

A monotonic elapsed clock advances longitude, wraps at 180 degrees and draws at most ten motion frames per second. The event loop retains its 250 ms poll when rotation is off or paused. It uses the production section and canvas layout to require a visible drawable globe at least 60 columns by 16 rows, pauses for another workspace, station context, filter/country editor or flat map, and pauses after five minutes without input. Pausing discards the motion anchor; resuming keeps longitude without catching up hidden time. Resize events do not renew the idle allowance. Repeated key events cannot repeatedly toggle motion.

Station markers use the existing geographic projection and per-cell clustering. The selected station retains its identity and `@` marker on the visible hemisphere, including when another station shares its cell. A selected station on the far hemisphere remains selected in the list and is correctly absent from the front-side drawing.

Deterministic clock fixtures test cadence, elapsed speed, wrap, manual interruption and resumed longitude. Isolated process fixtures test environment modes without mutating another test's environment. Production layout tests cover hidden modals and small terminals. The ordinary screenshot gallery remains deterministic. An explicit offline full-screen benchmark writes bounded color renders at 80 by 24 and 200 by 60 with measured timings; terminal I/O and native backend fidelity remain separate from this TestBackend evidence. Existing synchronous metadata requests can still delay input and drawing.
