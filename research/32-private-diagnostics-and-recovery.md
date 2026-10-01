# Private diagnostics and durable recovery

Reviewed: 2026-09-30. Status: primary-source research and proposed qualification work, with read-only inspection of current storage and backup code. No new runtime, dependency, telemetry integration or paid request was introduced. This supports C-42/C-43, acceptance R-59/R-60, [durable task workflows](../docs/design/task-workflows.md) and the [privacy plan](../docs/planning/09-security-privacy-and-release.md).

## Primary findings and design implications

### Private diagnostics

[OWASP's logging guidance](https://cheatsheetseries.owasp.org/cheatsheets/Logging_Cheat_Sheet.html) recommends excluding credentials and sensitive material, validating and encoding untrusted fields, restricting access, and testing exhaustion and logging failures. These application responsibilities support useful local diagnostics without requiring remote collection.

[OpenTelemetry's sensitive-data guidance](https://opentelemetry.io/docs/security/handling-sensitive-data/) emphasizes necessary attributes and review of instrumentation output. It cautions that hashes of predictable identifiers can be recovered by guessing. This is evidence for minimizing data at its producing boundary; it does not select an SDK, collector or exporter for Sigy.

**Inference:** use typed allowed fields rather than serializing full requests or model responses and redacting afterward. Keep events to operation identity, stage, bounded status/failure codes, timing and measured counts. IDs still reveal activity, so records need private access and finite retention. Raw URLs, arguments, native stderr, provider responses and error chains can contain secrets or hostile text. Review each before presentation or retention. Hashing station or topic identifiers does not establish anonymity.

Necessary task goals, plans and evidence remain protected application data under their own policy. Losing diagnostics must not prevent recovery. Explicit local debugging needs a byte cap, expiry and deletion path. Support bundles are locally assembled, bounded and inspectable; sharing requires explicit action. Enabling a provider grants no diagnostic-upload authority.

### Storage durability

[SQLite's synchronous reference](https://www.sqlite.org/pragma.html#pragma_synchronous) distinguishes application crashes from OS crashes and power loss. WAL with `FULL` adds commit synchronization that `NORMAL` omits. The same pragma reference explains that `quick_check` omits index/table consistency and uniqueness checks, while `integrity_check` does not check foreign keys. None establishes application accounting or external-effect correctness.

[SQLite's WAL documentation](https://www.sqlite.org/wal.html) requires same-host coordination, identifies WAL as persistent database state, and explains checkpoint starvation and growth with overlapping long readers. It documents a WAL-reset race fixed in 3.51.3 and later. Engine version and patch provenance matter alongside tests.

[SQLite's atomic-commit discussion](https://www.sqlite.org/atomiccommit.html) describes filesystem/storage assumptions and ineffective-flush failure modes. Its algorithm discussion concerns rollback mode; WAL uses another mechanism. Requesting synchronization does not prove every device honors persistence assumptions.

**Inference:** preserve WAL and `FULL`; record observed settings and runtime engine identity in qualification receipts. Budget catalog/WAL growth separately from media and diagnostics. Bound transactions and inspect checkpoints under long reads. Process-kill recovery is distinct from physical power-loss durability. The candidate five-second seal window is not measured maximum data loss. Catalog commits, media publication, directory entries and backup manifests require a tested publication sequence.

### Backup and corruption

[SQLite's `VACUUM INTO` reference](https://www.sqlite.org/lang_vacuum.html#vacuuminto) describes a consistent snapshot but warns that interruption can leave incomplete output. The [backup API](https://www.sqlite.org/backup.html) permits incremental catalog copying. Neither supplies an atomic snapshot of Sigy's separate media files without application coordination.

[SQLite's corruption guidance](https://www.sqlite.org/howtocorrupt.html) explains hazards from copying changing databases, separating required journals and unreliable locking. Arbitrary file copies and deletion of lingering WAL files are not recovery procedures.

**Inference:** retain offline ownership, verified catalog snapshots, media checks and manifest-last publication. Incomplete copies cannot become successful backups. Hashes detect accidental change relative to a manifest; unsigned manifests do not establish authenticity against replacement of files and hashes together. Preserve damaged evidence and a known backup on corruption. Automatic salvage cannot create authority, release uncertain liabilities or silently rewrite history. Backups need privacy and expiration policies. Logical deletion cannot establish erasure of prior backups, filesystem snapshots or device copies.

### Effect reconciliation

[RFC 9110 section 9.2.2](https://www.rfc-editor.org/rfc/rfc9110.html#section-9.2.2) defines idempotency by intended effect and constrains automatic retries of non-idempotent requests. A lost response is not proof that an action did not happen.

**Inference:** persist steps and stable action identities before dispatch. After restart, reconcile service jobs, receipts, generations and reservations before proposing another action. Fence stale attempts. Polling and resubmitting paid work are different decisions. Without proven replay semantics or proof of nondispatch, ambiguous submissions keep their liability and unresolved outcome. Durable state cannot depend on diagnostic lines or remote conversation handles. Extend [task recovery](../docs/design/task-workflows.md#recovery-and-completion) above the existing [job pool](../docs/decisions/0043-task-contract-and-job-pool.md), without another scheduler or ledger.

## Current evidence and alternatives

Read-only code inspection found [catalog opening](../crates/sigy-service/src/storage/mod.rs) requests WAL and `FULL`, enables foreign keys, performs a quick structural check, applies migrations and checks foreign keys and application audits. [Native provenance](../vendor/README.md) records retained SQLite 3.53.4 source hashes. [Backup and restore](../docs/decisions/0045-library-backup-and-restore.md) uses offline ownership, `VACUUM INTO`, media hashes and manifest-last publication. The [same-host v39 rehearsal](experiments/library-restore-v39.md) does not qualify another host, final release schema, interrupted migration or physical power loss.

The recommended first slice uses existing structured state and bounded local events. A remote telemetry stack adds unnecessary destinations and infrastructure. Online backup may reduce downtime but needs coordinated retention/media leases and contention evidence. A generic orchestration engine adds another state/queue lifecycle; first test finite workflows over canonical service state.

## Practical next evidence

These gates are proposed. Declare thresholds, workload, supported configurations and duration before measurement. [SQLite's testing methods](https://www.sqlite.org/testing.html) combine fault simulation, malformed inputs, boundary cases, fuzzing and regression fixtures. This methodological reference does not establish equivalent Sigy assurance.

| Qualification | Observable result required |
| --- | --- |
| Privacy | Plant canaries in credentials, URLs, prompts, transcripts, native output and errors. Across success, refusal, cancellation, crash and export, ordinary logs and default support bundles omit them. Enumerate intentionally stored task data separately. |
| Bounds and access | Flood failures/oversized fields, fill diagnostic allocation, deny writes and test another local identity. Capture does not depend on diagnostic success; dropped counts stay bounded and visible where possible. Required accounting/policy writes still fail closed. |
| Egress | Inspect actual connections with diagnostics enabled/disabled, including crashes. Permit only authorized source/provider/update operations. Test native network isolation separately; loopback endpoints and disabled exporters do not prove it. |
| Workflow recovery | Kill around every admission, dispatch and publication boundary. Compare effects, IDs, reservations and outcomes with an uninterrupted reference. Duplicate calls and stale workers cannot repeat effects or overwrite newer state. |
| Damage and disk pressure | On disposable copies, alter/truncate catalog, WAL and media, exhaust storage and interrupt publication. Preserve evidence, reject invalid state and recover from verified backups without resetting balances or fabricating coverage. |
| Restore and migration | Interrupt snapshot, media copy, manifest publication and migration. Verify current schema and restore onto a clean host with documented assets and unavailable secrets. Ambiguous liabilities remain held. |
| Sustained operation | Record host/filesystem/device, workload, WAL/diagnostic growth, resources, gaps and recovery time. I/O fault simulation, process death, OS crash and physical power loss are distinct evidence. Long service life remains a maintenance goal. |

No physical power interruption, user-data corruption, dependency change, cloud call or externally billed operation was performed for this research. Recheck primary sources before consequential storage, diagnostic or transport changes; retain exact versions with measurements.
