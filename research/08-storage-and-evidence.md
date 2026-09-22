# Storage, search, and evidence

Reviewed: 2026-09-22. Status: SQLite is selected and implemented for the durable catalog under the [foundation decision](../docs/decisions/0001-rust-foundation.md). Search and analytical extensions remain subject to separate qualification.

## Findings

SQLite documents atomic transactions and the filesystem assumptions behind them. Its guarantees do not automatically cover separate media files managed by the application. [Atomic commit](https://sqlite.org/atomiccommit.html).

SQLite WAL mode allows readers alongside a writer but still has one writer at a time, checkpoint behavior to manage, and restrictions involving shared memory and network filesystems. Sigy's service owns the catalog, with media stored separately. Workload and durability qualification remains specific to the tested profile. [WAL documentation](https://sqlite.org/wal.html).

SQLite offers a backup API for consistent database copying. A complete Sigy backup would additionally need a stable media manifest and its referenced objects. [Backup API](https://sqlite.org/backup.html).

FTS5 provides full-text indexing with several tokenizers and extension points. Its default Unicode tokenizer and English-oriented stemming option do not establish good search for every target language. [FTS5 documentation](https://sqlite.org/fts5.html).

A server database such as PostgreSQL offers another operational model with its own backup strategies. It introduces service administration that must be justified by requirements beyond a personal single-host installation. [Backup documentation](https://www.postgresql.org/docs/current/backup.html).

## Alternatives

| Approach | Strength | Cost |
| --- | --- | --- |
| SQLite transactional catalog plus media objects, selected | Compact deployment and clear ownership | Writer coordination, filesystem policy, application-level media reconciliation |
| DuckDB analytics over exported snapshots, possible later experiment | Columnar execution for large scans, joins, and aggregates | Another native engine, bounded resource use, snapshot freshness, and export cost |
| Server database plus media objects | Stronger fit for shared service operation | Additional installation, upgrades, accounts, backups, and resource use |
| Files and ad hoc metadata only | Easy inspection of individual artifacts | Harder transactional jobs, concurrent budgets, queries, migrations, and reference integrity |

Keep SQLite as the authoritative catalog. A server database should be reconsidered if shared multi-user operation creates requirements the current service cannot meet.

## SQLite and DuckDB reassessment, 2026-09-22

Sigy's immediate workload is frequent bounded state changes: reserve an exact budget, advance a job, publish a recording, or record an immutable revision. The existing [store](../crates/sigy-service/src/storage/mod.rs) uses WAL, full synchronization, foreign keys, migrations, and immediate transactions; [ledger constraints](../crates/sigy-service/src/storage/001-foundation.sql) protect immutable history. SQLite documents local application storage and serialized application-server access as suitable uses. Retaining this tested boundary is the current engineering choice; upstream descriptions do not establish Sigy's capacity. [Appropriate SQLite uses](https://www.sqlite.org/whentouse.html).

DuckDB supports ACID transactions and snapshot isolation. Its columnar execution targets analytical scans, joins, and aggregates; its native in-process mode allows concurrent writer threads within one writer process, with conflicts handled through optimistic concurrency control. These are useful capabilities, but they do not establish a benefit for Sigy's catalog transactions. No comparative Sigy workload benchmark has been run. [Analytical design](https://duckdb.org/why_duckdb), [transactions](https://duckdb.org/docs/current/sql/statements/transactions), [concurrency](https://duckdb.org/docs/current/connect/concurrency).

Version matters: the reviewed documentation identifies 1.5 as current and 2.0 as preview. The 2.0 preview includes triggers and server features. Reassess the released version when needed; this decision does not depend on a permanent absence of triggers or transactions in DuckDB. [Preview status](https://duckdb.org/install/preview), [2.0 feature preview](https://duckdb.org/2026/08/17/duckdb-20-highlights).

If measured historical reporting later needs another engine, compare SQLite queries with DuckDB over consistent, service-created snapshots or columnar exports. Keep those datasets derived and replaceable, with exact revision provenance and visible freshness. Measure export cost, query latency, peak memory, temporary disk, and impact on capture before adding a dependency. DuckDB's SQLite extension can read and write SQLite files, can load automatically, and warns about linking multiple SQLite copies. Any experiment must qualify that native boundary, explicitly control extension installation, and prevent writes to the live catalog. No engine, extension, or migration is added by this reassessment. [SQLite extension](https://duckdb.org/docs/current/core_extensions/sqlite).

## Evidence design

Use immutable committed media with checksums and a transactional manifest. Store original source metadata and its provenance, observed timestamps, processing revisions, and references from findings to precise media/text intervals.

Stable IDs outlive filenames and display names. Logical identities separate from content checksums because two identical clips can have different source/time provenance. Deduplicating storage must not erase those observations.

Retain original text separately from normalized search representations. User corrections and later model passes create revisions. A finding references the revision used, with a visible superseded status when appropriate.

Search can begin with lexical retrieval and structured filters, while semantic retrieval is evaluated against actual topic queries. Embeddings are versioned, rebuildable derivatives. Similarity is not evidence that a claim is supported.

### Retrieval evaluation

Compare lexical, multilingual embedding, and hybrid retrieval using labeled queries and expected supporting passages. Measure recall of relevant evidence, ranking quality, latency, index size, and indexing cost by language. Test names, numbers, diacritics, CJK text, translated queries, and noisy transcripts.

The first complete release needs reliable topic retrieval, but a dedicated vector database is not assumed. Choose the smallest architecture that satisfies measured retrieval and operational requirements.

## Failure and restore work

Inject failures before and after file finalization, catalog commit, queue insertion, result publication, retention deletion, and backup snapshot creation. Verify that replay/reconciliation is idempotent.

Test full disk, read-only library, missing mount, corrupted media, stale locks, second service start, long paths, Unicode names, interrupted migration, and old backup restore. Do not automatically replay historical paid requests or revive expired schedules during restore.

## Near future

Keep exports self-describing and media formats independently readable. Add remote object stores only when a concrete deployment needs them and their consistency, credentials, costs, and partial-upload recovery are specified. Source provenance and model revision history should survive backend replacement.
