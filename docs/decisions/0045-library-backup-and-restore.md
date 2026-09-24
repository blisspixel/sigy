# Library backup and restore

Date: 2026-09-24. Status: implemented and tested on Windows x86_64 at catalog v30. This is the first part of roadmap operation 38. The operation stays open until it is rerun against the release schema on a clean host and an interrupted migration is recovered from a backup.

## Decision

`sigy library backup DIR` copies a library into a new directory while this process owns the library, so no service can write during the backup. It refuses to run while the service holds the library. The backup holds:

- `catalog.sqlite3`, a transactionally consistent `VACUUM INTO` snapshot taken after `PRAGMA integrity_check` returns `ok`;
- `media/<key>.media`, a copy of every retained published interval the catalog vouches for, hashed while copying and compared with the catalog's size and SHA-256;
- `manifest.json` (`sigy-backup-v1`): schema version, creation time, the catalog's size and hash, and every media object's key, size and hash. It is written last, so a failed backup has no manifest and is not a backup.

A missing or changed source object fails the backup. Unpublished capture parts, temporary scratch, models and exported sidecars are not included.

`sigy library verify-backup DIR` checks the catalog and every media file against the manifest and refuses extra, missing, repeated or changed files, a malformed or oversized manifest, an unknown format or a newer schema.

`sigy library restore DIR --into NEW` verifies the backup, copies it into a staging directory beside the destination, verifies each copy again, opens it as a library (which runs migrations for an older schema and the catalog audits), checks integrity, and confirms that every retained interval in the restored catalog is present with the same size and hash. Only then is the staging directory renamed into place. Any failure removes the staging directory and leaves no library. The destination must not exist.

After restore, the next service start applies normal recovery: captures and jobs that were running at backup time become interrupted, schedules do not backfill missed windows, and uncertain paid liabilities stay reserved.

## Evidence

Tests on temporary libraries cover a round trip that reproduces the recordings and every media hash, refusal of existing destinations with nothing overwritten, a changed or missing source object, tampered, missing and extra media, a changed catalog, a malformed manifest, and a manifest that omits an object the catalog needs. A rehearsal on a working library with live station recordings, transcripts and translations backed up 15 objects (11.2 MB) in about 3 seconds, verified, restored, and showed the same transcripts and translations with a clean `doctor` report. A backup attempt while the service held the library was refused with nothing written.

## Limitations

Backup is offline: stop the service first. Hashes detect corruption, not tampering; the manifest is not signed. There is no incremental backup, compression or encryption. Restore onto another host and a physical power-loss test have not been run. The rename into place is atomic only on one filesystem.
