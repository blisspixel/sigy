# 0023: Measured recording intervals

Date: 2026-09-22. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

One published file is one interval. The bounds are the decoded duration and the published byte length, both starting at zero. The requested window stays on the recording as `duration_seconds`. It is not the interval. A reserved or interrupted attempt has no interval, so its part file is not airtime.

Opening a catalog at schema v15 projects every already published row. A new publication writes the interval in the same transaction as the file hash and decoded duration. The interval is immutable and retained with the recording history. Deleting the file releases quota and leaves the measured bounds in the catalog.

Catalog schema advances to v16. Local IPC stays v16 because no client field was added. This does not seal a running capture, play multiple segments, or exit stage 4.
