# 0005: Service-owned recordings and rolling retention

Date: 2026-09-21. Status: implemented initial increment, with qualification limits below.

This records the initial recording profile. [Authorized redirects](0007-authorized-redirects.md) subsequently extends acquisition, adds route provenance and advances recording exports to envelope v2.

## Decision

The existing controller owns finite HTTP audio recording workers. CLI exit does not cancel them. Source registration remains separate from dispatch. The catalog atomically creates capture intent, records the starting generation, and reserves the entire requested byte ceiling. Exact request replay never dispatches again, including after failure or restart.

Schema v4 adds one DVR policy and recording records. Defaults are 14 days and 50,000,000,000 media bytes, with a 256 MiB free-space floor. Configuring a trusted installed decoder is explicit. Temporary recordings expire by age, become eligible after acknowledged processing, or are evicted oldest-first when new recording reservations need quota. Kept and archived media are exempt from automatic deletion and still consume quota. Archive is a retention designation, not a second copy or backup.

The service sweeps at startup and every minute, in batches of at most 64. Active workers are protected. If protected material or active reservations exhaust capacity, new work fails with a storage error. Deletion records intent before removing files and releases quota only after removal. Startup resumes pending deletions. Failed and interrupted captures retain their full reservation until deletion; this deliberately overestimates possible partial bytes. Deleting interrupted media also cancels its pending intent transactionally, without making the recording ID reusable.

Files use generated keys under the private library media directory. A worker creates a new staging file, receives bounded original bytes, flushes and syncs them, validates decoding, computes SHA-256, and renames the file. Only the catalog owner can publish completion against the admitted generation. A crash before catalog publication leaves an interrupted reservation and recoverable bytes, not a false completed recording. Originals are never transcoded in place.

Hickory Resolver replaces blocking OS name lookup while retaining system-configured nameservers and the existing destination policy. HTTP still rejects playlists, content decompression, and automatic proxies/retries. Redirects are opt-in in a later decision. Interleaved ICY metadata fails before a body write unless `record start --icy` is set; see [ICY observations](0013-icy-observations.md). Direct MP3, AAC, FLAC, Ogg and WAV MIME types enter the decoder gate. MIME labels alone cannot complete a recording.

The decoder is an explicitly configured FFmpeg executable. It receives a local file handle on stdin, a forced demuxer, and only the pipe protocol. It receives no station URL. Decode output is discarded; bounded progress must report actual audio and successful completion. Process supervision limits wall time to 30 seconds, progress to 64 KiB, threads to one for decoding/filtering, and individual allocations to 16 MiB. Windows uses a kill-on-close job; Unix uses a process group. Two capture lifecycle slots bound concurrent capture and validation.

## Interfaces

`record start`, `stop`, `list`, `show`, `path`, `metadata`, `keep`, `archive`, `temporary`, `processed`, and `delete` share controller operations. `dvr configure`, `status`, and `prune` share the same storage policy. IPC is v4; stop older controllers before replacing their binary. `record metadata` exports a versioned JSON snapshot suitable for a sidecar. It contains an origin, not credential-bearing URL paths or queries.

## Evidence and limits

The native-media verification script exercises real CLI processes, a local WAV server, an installed decoder, successful publication, exact byte preservation, metadata export, processing acknowledgment, protected retention, cleanup, rejection of non-audio bytes labeled as WAV, and service kill/restart during partial reception. Transaction tests cover competing reservations, migration rollback, replay, stale workers, interrupted liability, completion/deletion accounting, and age/pressure protection. HTTP fixtures distinguish timed/stopped recordings from failed or empty transfers. Current results belong in [progress](../development/progress.md).

This is not continuous segmented DVR, listening during capture, a station directory, translation, or an RF receiver. A radio attempt is limited to 15 minutes and 256 MiB. An episode enclosure uses the separate ceiling in [one episode enclosure](0019-episode-enclosure.md). Unsupported streams fail explicitly. Codec availability depends on the configured decoder; broad codec and OS qualification remains open. FFmpeg is not bundled, downloaded, or covered by the Rust dependency audit. Users must maintain that executable.

The allocation setting is not an aggregate process-memory ceiling. Hard memory/CPU sandbox profiles, Unix parent-death qualification, decoder fault corpora, and physical power-loss testing remain gates for unattended release support. The free-space check includes outstanding full reservations and a floor, but cannot reserve disk capacity against unrelated programs. The 50 GB limit covers managed media and reservations; catalog/WAL, exported files, models, and future derivative caches require separate accounting. Rename/directory durability on Windows still needs physical failure qualification. A stored checksum is integrity evidence at publication; arbitrary external edits to files require later reconciliation.

See [recording metadata](../design/recording-metadata.md), [source boundary](0004-source-authority-and-http.md), and [research](../../research/26-recording-discovery-and-rf.md).
