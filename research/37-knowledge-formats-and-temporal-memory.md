# Knowledge formats and temporal memory research

Reviewed: 2026-10-03. Status: primary-source review and proposed design refinements. No knowledge adapter, semantic graph, general agent memory or new qualification is established by this note. Recommendations extend the existing [analysis contract](../docs/planning/10-analysis-and-knowledge.md), [topic monitoring](../docs/design/topic-monitoring.md), [task workflows](../docs/design/task-workflows.md) and [scaling architecture](../docs/design/scaling-architecture.md).

## Article, specification and revision pins

[The Google Cloud article](https://cloud.google.com/blog/products/data-analytics/how-the-open-knowledge-format-can-improve-data-sharing), published 2026-06-12, introduces OKF v0.1 as Markdown files with YAML metadata and links. It describes its producer and viewer as proofs of concept and the format as a starting point. This is useful interoperability direction, not evidence that automated wiki maintenance preserves correctness or that a particular deployment scales.

The article's linked [knowledge-catalog directory](https://raw.githubusercontent.com/GoogleCloudPlatform/knowledge-catalog/main/okf/README.md) is now a frozen copy. Its notice redirects readers to the [canonical OKF repository](https://github.com/GoogleCloudPlatform/open-knowledge-format). The reviewed [v0.2 specification](https://github.com/GoogleCloudPlatform/open-knowledge-format/blob/0b87c52c6ef999286c745e19998fdfcd03d5dbee/SPEC.md) is pinned to commit `0b87c52c6ef999286c745e19998fdfcd03d5dbee`, committed 2026-08-21T19:31:43Z. The pin was obtained from the official repository's file history and its contents were inspected on the review date.

The specification retains path-based concept identity and required `type`. Optional provenance uses source IDs and claim footnotes; production and verification are separate fields. Lifecycle metadata describes freshness. Additional fields are allowed. Its advisory trust tiers are not access control. Deeper external lineage and attester sandboxing remain outside or deferred; its version history explicitly notes two v0.1 field changes despite the minor version increment. An adapter needs explicit compatibility fixtures and a Sigy metadata profile.

Recommendation: use OKF as an optional interchange projection. Keep canonical IDs and revision identities independent of filenames. Pin the specification and declared export profile. File moves, rewritten prose, verifier strings and popularity counters cannot establish evidence identity, correctness or authority.

## Logical roles before additional systems

The current planning contract already keeps the transactional catalog responsible for jobs and budgets while making search and topic projections rebuildable. Topic monitoring separates observations, finding revisions, relationships and frozen briefings. Build on those boundaries.

| Role | Proposed contents and boundary |
| --- | --- |
| Evidence memory | Immutable source revisions, observations, clocks, transcripts, translations, corrections, profile identities and execution receipts |
| Claim memory | Attributed propositions and entity hypotheses with exact supporting spans, competing claims, derivation methods and revision history |
| Navigation memory | Lexical indexes, optional embeddings, adjacency lists, timelines and topic pages; rebuildable projections |
| Task context | One bounded, frozen selection of evidence and claims supplied to one admitted execution |
| User preferences | Explicit user-supplied choices with purpose, revision and retention rules; observations cannot invent preferences |
| Procedural memory | Separately authorized and versioned skills or playbooks; evidence imports cannot install or revise them |

These roles can initially share typed SQLite storage and the existing service. A graph is a relationship model, not a requirement for another database. Introduce another storage system when measured traversal, write concurrency or deployment requirements justify its operational cost, then require conformance, migration and restoration evidence. Multiple execution workers do not themselves require multiple stores of truth.

## Wiki practices and their limits

[The original wiki proposal](https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f) distinguishes immutable raw sources, generated linked pages and conventions. Its ingest, query and lint workflows accumulate summaries and cross-references; indexes support progressive navigation and logs expose updates. The text presents an adaptable idea and experience at moderate personal-library scale, not a benchmark or production guarantee.

Recommendation: retain the readable linked-page experience, but publish a named projection generation through the service. Each page identifies its exact dependency set, coverage and derivation profile. Ingesting a source can propose page changes; neither source text nor generated pages can modify skills, grants or policy. A lint pass checks structural references mechanically and records semantic suspicions as proposals. An answer filed into the wiki remains derived from its original evidence and cannot count as new corroboration.

Keep original scripts and unknown representations navigable. Translation target, presentation locale and entity alias are separate identities. English is the default translation direction, not a universal intermediate representation. A translated page points to its exact original and target revisions. Unresolved names, mixed languages, symbolic material and unfamiliar representations remain explicit rather than being forced into an English entity label.

## Temporal graphs and actual implementation evidence

[Graphiti's official repository](https://github.com/getzep/graphiti) describes incremental episode ingestion, hybrid retrieval and temporal relationships. Its current framework requires Python and a supported graph backend, with model-assisted extraction. The repository warns that smaller or incompatible models can fail structured ingestion. This is a design reference, not a dependency choice under Sigy's Rust-only foundation and bounded local baseline.

The inspected [edge implementation](https://github.com/getzep/graphiti/blob/3c9547cfbe12ddafa6b753a8125e34a7796d353e/graphiti_core/edges.py) is pinned to commit `3c9547cfbe12ddafa6b753a8125e34a7796d353e`, committed 2026-04-18T19:47:29Z. It binds semantic edges to episode IDs and carries creation, expiration, validity, invalidity and reference timestamps. Those fields demonstrate a concrete temporal representation; they do not prove that inferred facts or validity intervals are correct.

[The January 2025 Zep paper](https://arxiv.org/html/2501.13956v1) evaluates conversational retrieval. It reports smaller contexts and aggregate improvements on LongMemEval, but also weaker assistant-message retrieval, limitations of the smaller DMR benchmark and unavailable comparative results for one attempted system. Episode-to-derived-edge provenance connections were not directly evaluated. Its results do not qualify multilingual radio extraction, contradiction detection, source independence or Sigy's local resource profile.

Recommendation: adopt explicit clocks and lineage as contracts before experimenting with automatic entity merging or semantic invalidation. Preserve broadcast disagreement rather than treating the latest extracted sentence as a replacement fact.

| Time or status | Required distinction |
| --- | --- |
| Capture and media time | Received interval and mapping uncertainty; not assumed emission time |
| Assertion and referenced time | When a statement was made and the event time it claims; preserve ambiguous dates, precision and time zone |
| Catalog time | When Sigy admitted or published a record; append-only history supports what was known at a past cutoff |
| Derivation time | When an exact profile produced an interpretation from exact inputs |
| Proposed validity | Attributed interpretation of a state interval; may remain unknown |
| Revision and lifecycle | Correction, retraction, stale dependency, media expiration and privacy deletion are distinct outcomes |

Support both an event-time question and a library-knowledge-cutoff question. Late arrivals must not rewrite what an earlier briefing knew. A corrected transcript supersedes a derived revision, while a conflicting broadcast creates competing claims. Quotation, hypothesis, negation, retraction and temporal change need distinct relationships. Keep relation origin and evidence visible. Unknown independence remains unknown; an outlet count or user assertion about independence is not independent corroboration by itself.

[W3C PROV-DM](https://www.w3.org/TR/prov-dm/), a recommendation published 2013-04-30, provides stable entity, activity and derivation concepts for exchanging lineage across systems. It supports provenance assessment rather than establishing factual truth. Use it as an optional mapping reference without requiring RDF storage or changing Sigy's canonical evidence model.

## Retrieval qualification and bounded operation

[LongMemEval](https://arxiv.org/abs/2410.10813), revised 2025-03-04 and published at ICLR 2025, separates extraction, multi-session reasoning, temporal reasoning, knowledge updates and abstention. Its indexing, retrieval and reading breakdown is useful evaluation structure. Its conversational questions do not establish broadcast-domain performance or multilingual qualification.

Recommendation: qualify retrieval and interpretation separately. Freeze exact query scope, expected evidence IDs, language and target directions, profile hashes and acceptance criteria. Measure recall and false retrieval, unsupported conclusions, citation fidelity, temporal cutoff correctness, abstention, tail latency, memory and build cost. Include late arrivals, corrections, mixed scripts, unresolved aliases, absent evidence, syndicated copies, conflicting sources and expired media. Use independent original sources rather than repeated translations or generated summaries as evaluation units. Preserve missing and unsupported outcomes in denominators. Newly arranged human review is not a prerequisite; reproducible references and calibrated checks still cannot establish unmeasured semantic quality.

Every retrieval envelope should expose projection generation and lag, canonical IDs and revisions, original and requested target text, retained or unavailable evidence, coverage, uncertainty, and the bounds reached. Bound scanned rows, graph depth, fan-out, output bytes, context tokens, wall time and allocation separately. Context construction cannot silently perform new analysis or obtain a paid route. Projection backfills reuse durable jobs, finite authority, resource admission and generation fencing. A restart or correction invalidates stale dependencies without dispatching an unbounded rebuild.

Deleting evidence for privacy requires explicit policy over derived indexes, cached contexts, exports and backups. Ordinary media expiration does not imply transcript deletion. A restored backup must reapply the applicable deletion history before serving projections; an old export cannot be silently recalled from another recipient. These are acceptance requirements, not current deletion guarantees.

## Export-first adapter and small packages

| Package | Proposed result and acceptance evidence |
| --- | --- |
| KM-01: Memory and time contract | Typed roles, canonical IDs, revision dependencies and time semantics; independent late-arrival, correction, contradiction and deletion examples |
| KM-02: Topic projection | Bounded read-only adjacency and timelines from existing exact findings; watermark, lag, replay, crash recovery and byte/scan limits |
| KM-03: Frozen context | A finite evidence envelope with identity and omission receipts; injection, multilingual, stale-revision and cutoff fixtures |
| KM-04: OKF export | One named generation rendered with stable paths, per-claim references and explicit Sigy metadata; pinned-version conformance, deterministic output, redaction and round-trip identity inspection |
| KM-05: Semantic experiments | Optional, profile-bound entity and relationship proposals evaluated against independent references; no authority or automatic truth promotion |

Start KM-04 with read-only exports and no executable computation. Publish complete bundles through staging and atomic replacement with a digest manifest; failed export leaves the previous generation intact. Machine consumers receive stable structured envelopes alongside readable Markdown. Page counts, graph size and export bytes stay finite. The initial terminal experience requires no browser or external wiki application.

A later importer treats YAML, links, Markdown instructions, declared computations and verifier strings as untrusted content. Bound parsing, nesting, aliases, files and total bytes; resolve paths inside the selected bundle and reject escape or symlink traversal. Accept unfamiliar concept types as data while preserving unresolved references. Reading an imported concept never fetches its URLs, runs its code, installs a skill, grants collection, changes budgets or attests its own claims. Any future approved computation must use an independently authorized typed service operation, immutable dependency identities and exact receipts. Structural conformance and a successful hash check do not qualify semantic correctness.

The recommended order preserves the useful wiki and graph ideas while keeping evidence, authority and resource accounting in the existing service. Additional backends remain measured alternatives rather than prerequisites for exceptional engineering.

The [canonical storage and memory contract](../docs/design/storage-and-memory.md) owns durable role, placement and time semantics; [ST/AR/KM packages](../docs/development/reliability-and-scale.md#storage-and-memory-increments) own implementation acceptance. General knowledge-as-of reconstruction is proposed: use a committed revision/generation cutoff or named snapshot, not wall-clock timestamps alone. Initial topic edges express exact membership/dependencies and established mechanical repetition; semantic support, contradiction and identity require their separately qualified or explicitly supplied origin.
