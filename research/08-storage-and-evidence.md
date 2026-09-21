# Storage, search, and evidence

Reviewed: 2026-09-20. Status: storage research and proposed data contracts. No database selected.

## Findings

SQLite documents atomic transactions and the filesystem assumptions behind them. Its guarantees do not automatically cover separate media files managed by the application. [Atomic commit](https://sqlite.org/atomiccommit.html).

SQLite WAL mode allows readers alongside a writer but still has one writer at a time, checkpoint behavior to manage, and restrictions involving shared memory and network filesystems. A single service writer with media outside the database is a credible candidate topology, subject to workload and durability testing. [WAL documentation](https://sqlite.org/wal.html).

SQLite offers a backup API for consistent database copying. A complete Sigy backup would additionally need a stable media manifest and its referenced objects. [Backup API](https://sqlite.org/backup.html).

FTS5 provides full-text indexing with several tokenizers and extension points. Its default Unicode tokenizer and English-oriented stemming option do not establish good search for every target language. [FTS5 documentation](https://sqlite.org/fts5.html).

A server database such as PostgreSQL offers another operational model with its own backup strategies. It introduces service administration that must be justified by requirements beyond a personal single-host installation. [Backup documentation](https://www.postgresql.org/docs/current/backup.html).

## Alternatives

| Approach | Strength | Cost |
| --- | --- | --- |
| Embedded transactional catalog plus media objects | Compact deployment and clear ownership | Writer coordination, filesystem policy, application-level media reconciliation |
| Server database plus media objects | Stronger fit for shared service operation | Additional installation, upgrades, accounts, backups, and resource use |
| Files and ad hoc metadata only | Easy inspection of individual artifacts | Harder transactional jobs, concurrent budgets, queries, migrations, and reference integrity |

An embedded catalog is the leading design hypothesis for the confirmed single-installation product. SQLite is a candidate, not a selection. A server database should be reconsidered if shared multi-user operation becomes a requirement.

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
