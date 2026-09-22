# 0025: Segment seals on one running job

Date: 2026-09-22. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

A radio capture whose byte budget is at least 32 MiB seals while the job stays running. The open segment ceiling is 32 MiB, inside the 256 MiB radio cap. The seal closes on that ceiling or on 5000 ms of receive time, whichever comes first. That 5000 ms bound is the candidate uncommitted window. It is not a measured durability result.

Admit escrows the whole finite byte budget in integers, with the same quota failure as one reservation. Opening a segment assigns one ceiling from that escrow. Sealing charges the actual bytes and returns the unused difference to the escrow. The next segment opens only when a full ceiling remains. Renewal extends the lease on the same socket and does not open a second upstream request. A stale job, revision, and generation cannot seal.

Each sealed segment is its own file. The decoder checks that local file, and the hash is of those bytes, before the interval is published. The open tail is not an interval. A capture below 32 MiB, an HLS recording, and an episode download stay one file. `listen file` still plays one retained file and does not walk a multi-segment timeline.

Catalog schema is v17. Local IPC is v17. This does not journal gaps, play many segments, or exit stage 4.
