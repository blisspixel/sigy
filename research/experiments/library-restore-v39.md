# Same-host library restore at catalog v39

Run: 2026-09-30 on Windows 11 x86_64, using the existing development executable at source checkpoint `731f2e3`. Scope: a disposable copy of the retained live-station pilot library. No service, network request, model inference, or paid request ran.

## Method and results

The earlier `sigy-backup-v1` backup at catalog v30 was verified, restored into a new directory, and opened through the existing library migration path to v39. The migrated library was backed up into another new directory, verified, and restored again. Each command completed successfully.

Both manifests listed the same 15 retained media objects, totaling 11,210,170 bytes. Every key, byte count and SHA-256 stayed identical across migration, and each restored media file independently matched the v39 backup manifest. The v39 catalog snapshot contained 970,752 bytes, with SHA-256 `9cab83bbb69973923870b547846019567e8c2b3c71fefa6e8b332c36078aeca3`.

`sigy doctor` on the restored library passed its catalog, provider, budget, decoder, quota, capture, and recognition checks. It reported attention for an aged directory cache; it did not refresh that cache. Recognition pace and worker cost were unmeasured because this historical library lacks those later measurements. Restore did not invent them.

Local command outputs and the two restored libraries are retained in gitignored `.agents/restore-rehearsal-20260930-091517/`. Media and catalog contents are not published.

## Limits

This is one same-host migration and round trip of an existing working library. It does not establish clean-host restoration, physical power-loss behavior, interrupted migration recovery, a release schema, or preservation of populated monitor briefing records absent from this older input. [Backup and restore](../../docs/decisions/0045-library-backup-and-restore.md) and roadmap operation 38 remain partial.
