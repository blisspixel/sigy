# 0024: Doctor and cache freshness

Date: 2026-09-22. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

`sigy doctor` is a read-only preflight. It opens the library, or asks the running service, and does not refresh, delete, record, or contact a network. Each check is ok, attention, or blocked. Attention names the next command. Blocked exits with an error. `--strict` also exits when any check needs attention.

The directory cache is partial. Stations older than 24 hours stay searchable and are reported as stale. Favorites stay. Unseen stations stay, because one page cannot prove that the directory removed them. A failed or interrupted refresh leaves the previous cache. One refresh may run. The suggested command is `radio refresh` with a new id. Doctor does not send it.

Podcast snapshots use the same 24 hour attention window. The suggestion does not download an enclosure. A configured FFmpeg path that is no longer a file is blocked, and recording will not start. A library with no decoder configured can still search and inspect. This does not add a saved refresh schedule, and it does not exit stage 4.
