# Agentic analysis and insight generation

Reviewed: 2026-09-20. Status: research supporting proposed application contracts. No agent framework was selected for the monitor loop. On 2026-09-22 the local `sigy mcp` server and Agent Plugins 1.0.0 package were added for existing commands. They do not implement the monitor loop below.

## Current capabilities and evidence

Ollama documents tool calling, structured outputs, and embeddings as separate capabilities. Verify the actual configured model/runtime combination. A model proposing a tool call still needs an application to validate and execute it; a schema does not establish factual accuracy. [Tool calling](https://docs.ollama.com/capabilities/tool-calling), [structured outputs](https://docs.ollama.com/capabilities/structured-outputs), [embeddings](https://docs.ollama.com/capabilities/embeddings).

OpenRouter documents structured-output and routing features whose availability depends on models and providers. Treat endpoint-specific support as a contract, especially for newer decision endpoints. [Structured outputs](https://openrouter.ai/docs/guides/features/structured-outputs). The separate [decision-model research](16-decision-models-and-classifiers.md) covers Jev and local alternatives.

The distinction between predefined workflows and agents dynamically choosing steps is useful here. Start with explicit application workflows and add adaptive decisions where they improve measured results; a framework is not a substitute for well-defined tools and evaluation. This is a design inference from a foundational engineering guide, not a September 2026 benchmark. [Building effective agents](https://www.anthropic.com/engineering/building-effective-agents).

The current published MCP revision reviewed here is **2026-07-28**, with stateless, self-contained protocol requests. It is a candidate interoperability boundary for external clients and tools. It does not provide Sigy's job persistence, budget enforcement, or process sandbox. [Protocol specification](https://modelcontextprotocol.io/specification/2026-07-28). The August 22 roadmap describes subsequent work; future items are not assumed available. [Protocol roadmap](https://blog.modelcontextprotocol.io/posts/mcp-roadmap/).

MCP's security guidance covers boundaries including confused-deputy behavior, token handling, and server-side network requests. External servers and their returned content remain separate trust domains. [Security best practices](https://modelcontextprotocol.io/docs/2026-07-28/tutorials/security/security_best_practices).

## Proposed bounded monitor loop

1. Compile the user's goal into a versioned monitor specification with source scope, languages, schedule, resource allowances, freshness, and spending policy.
2. Discover and rank candidates using metadata and retained observations. Reserve exploration capacity so a monitor can find new sources.
3. Submit bounded collection/processing proposals to the deterministic executor. The executor alone admits work and changes source assignments.
4. Retrieve relevant original and translated passages with coverage metadata. Use inexpensive classification where it demonstrably improves selection.
5. Group reports of the same event while retaining station/feed provenance and contradictions.
6. Produce findings and update the topic overview with explicit supporting intervals, uncertainty, and changes since the previous report.
7. Evaluate usefulness and source quality, then propose bounded adjustments. Stop or wait when limits, deadlines, or retry rules require it.

The first release does not require an unconstrained conversational agent continually generating plans. Event-driven processing and bounded periodic review may meet the monitoring requirement with less cost and more predictable recovery. Compare them against a more dynamic loop before choosing orchestration machinery.

## Evidence and insight contracts

Each material claim needs a type: directly observed measurement, source-reported assertion, inferred relationship, or generated summary. A station saying something does not make it independently verified. Store event time when stated separately from capture time, and preserve uncertainty about both.

Validate that cited IDs and intervals exist and were available to the model. This checks reference integrity, not whether a passage semantically supports a claim. Evaluate semantic support with published reference cases, deliberately altered and contradictory passages, deterministic citation checks, and calibrated independent model review where useful. Record disagreements and unresolved claims. New human reviewers are not a release dependency, and a model's own judgment cannot be the sole gate.

Retrieval should consider original-language text, translations, lexical matches, and optional embeddings. Preserve passage identity across these representations. Test cross-language names, local terms, negation, and important passages with poor ASR. A summary should not recursively summarize only previous summaries; periodically rebuild from retained evidence to detect accumulated distortion.

Detect duplicated and syndicated content separately from independent reporting. Ten stations carrying one bulletin are ten observations of distribution, not ten independent confirmations. Source selection and processing coverage must accompany comparisons over time.

Novelty and importance are different judgments. A new detail may be unimportant, while a repeated development may matter to the user. Record the criterion used rather than collapsing both into a mysterious global relevance score.

## Authority and failure boundaries

Radio audio, transcripts, station descriptions, retrieved documents, tool responses, and notebook contents are untrusted model input. Delimit them, but do not depend on prompting alone for protection. Models cannot authorize network destinations, expand retention, install components, execute shell commands, reveal secrets, increase budgets, or grant themselves new tools.

A planner receives only the tools needed for its current task. Tool arguments use bounded schemas and explicit resource IDs. The executor rechecks current policy at dispatch, because a previously valid proposal may become stale. Every paid operation and retry passes through the shared ledger.

Persist goals, accepted operations, evidence references, checkpoints, and reasons for source changes. Do not depend on a provider conversation ID or hidden model reasoning for restart correctness. Record concise decisions and structured traces without placing credentials or entire sensitive transcripts into ordinary logs.

Model/provider failure should degrade the affected analysis stage visibly. It must not stop healthy capture, fabricate a completed report, or trigger an unapproved remote fallback. A report deadline can produce a partial report with coverage, not an invented complete one.

## Evaluation and release evidence

Evaluate the whole monitor on labeled time windows with hidden reference findings and supporting recordings. Separate collection coverage, processing coverage, retrieval recall, classifier recall, claim support, report usefulness, freshness, and cost. Include false positives and costly missed events.

Replay source outages, duplicates, multilingual switches, corrections, topic drift, adversarial passages, exhausted budgets, worker crashes, and model upgrades. Compare a deterministic schedule with adaptive discovery using equal resource allowances. An adaptive system should demonstrate better relevant coverage, not simply produce more text or consume more inference.

Version prompts, tool schemas, taxonomies, model identities, and policies. Freeze an evaluation subset for comparable upgrades and rotate a separate challenge set to avoid optimizing exclusively for known fixtures. Observe per-language regressions, rare-topic misses, and human corrections after deployment.

## Near future

Watch improved local speech and decision models, reliable structured tool interfaces, and portable interoperability standards. Prefer stable application contracts and replayable evidence over dependence on one agent framework. External agent access can begin with explicitly configured read-only search/report operations later; neither MCP nor external autonomy is required to deliver the first-release monitor.
