# Durable task workflows

Updated: 2026-09-30. Status: confirmed product direction with a proposed complete workflow contract. The [v41 increment](../decisions/0066-durable-task-workflows.md) implements immutable task scope and service-observed checkpoints. The [v42 publication slice](../decisions/0067-task-evidence-execution.md) adds explicit bounded delegation for task-owned literal findings and an exact-membership briefing; its verification status is recorded in that decision and active work. General task orchestration, task-owned capture, A2A, local planning-model transport and live paid-model transport are not implemented. This complements [topic monitoring](topic-monitoring.md), [architecture](../planning/02-architecture-and-data.md), [provider policy](../planning/07-providers-and-cost-policy.md) and [assurance](../planning/04-assurance-and-validation.md).

## Product requirement

A user states a task and its boundaries. Sigy uses its supported collection, processing and analysis capabilities to carry it through to a useful, checkable result. The user should not need to assemble individual tool calls. Local models with zero metered inference fees are a required path; explicitly configured paid providers are optional. Skills, the agent plugin, MCP and A2A are integration targets around the same application operations. Transport compatibility and tool execution do not establish task competence.

This requirement preserves the first-release radio, translation and monitoring scope. It does not move RF hardware, music identification or later workbench features into that release. An unavailable capability produces an explicit partial, blocked or unsupported outcome. The [first task increment](../decisions/0066-durable-task-workflows.md) defines immutable scope and service-observed checkpoints. Bounded evidence publication follows it; broader collection orchestration and planning transport remain separate increments.

## Current gap

The existing [agent plugin](../decisions/0021-agent-plugin.md) packages one skill and fixed stdio MCP commands for one library. It can inspect a cached directory, record registered sources and run configured local recognition and translation profiles. It cannot create a complete fresh radio-monitoring setup, publish findings or briefings, or orchestrate a durable general task. Its intentional authority limits remain in force until a scoped delegation contract is implemented. Provider configuration and offline billing fixtures do not supply live planning or provider transport. Local native recognition and translation are separate implemented capabilities.

## Proposed workflow contract

A workflow stores the user's goal, immutable accepted scope, declared success criteria, source and destination policy, deadlines, resource ceilings, paid allowance, permitted operations, model/skill versions, concise decisions, admitted job references, checkpoints, evidence and outcome. This is a user-task record above the existing worker [TaskSpec and job pool](../decisions/0043-task-contract-and-job-pool.md); it is not a replacement worker contract or another scheduler.

1. Interpret the request into typed task fields and identify prerequisites. Apply saved user limits and make consequential assumptions visible. Existing authorization permits routine progress inside those limits without repeated prompts. Missing authority cannot be manufactured from a model answer.
2. Select a versioned workflow or skill and propose bounded steps. Skills describe capabilities and task methods; they cannot expand permissions, install components or change allowances.
3. Have the service validate each proposed operation against current policy before admission. Source registration, schedules, monitoring, findings and reports need explicit scoped delegation before agents can reach their mutations. Do not expose unrestricted CLI or shell execution as a shortcut.
4. Execute through the existing acquirer, catalog, supervisor, job pool, resource admission and exact ledger. Acquisition and capture remain independent of a slow or failed planner.
5. Inspect actual results and coverage. Model output may propose interpretation, comparison or the next step. Validate reference existence separately from semantic support, preserve alternatives and uncertainty, and never convert tool success into proof of a supported conclusion.
6. Publish a cited result or a clearly stated partial outcome. Record why collection or analysis stopped, what remains unresolved, missing coverage and the actual resource and cost effects.

## Recovery and completion

The service owns progress after any client or external harness exits. Persist accepted actions and their idempotency identities before effects, then reconcile actual durable job and receipt states after restart. Fence stale attempts and use the existing leases and worker generations. Retry only eligible operations under the same finite policy. An ambiguous paid submission retains its liability and is not blindly resent. Missed civil windows remain missed.

Resume from necessary structured state and retained evidence, not hidden model reasoning or a provider conversation handle. Model, prompt or skill changes are explicit versions and cannot rewrite earlier decisions. Cancellation, policy revocation, retention expiry and unavailable input remain visible. Completion requires checked artifacts and the declared outcome criteria; unsupported quality is reported unresolved even when every process exited successfully.

## Model and interoperability roles

| Surface | Responsibility | Required evidence |
| --- | --- | --- |
| Built-in local planner | Interpret and advance bounded tasks using a qualified local model | Tool/schema correctness, useful task completion, measured host fit, no metered dispatch and verified destinations |
| Optional hosted planner | The same task contract through an explicitly enabled provider such as OpenRouter | Model/endpoint capability, permitted data flow, finite request and loop bounds, exact reservations and ambiguous-charge recovery |
| Skills and plugin | Package reusable workflow guidance and expose supported capabilities to another harness | Versioned manifests, limited operations, real end-to-end fixtures and accurate capability reporting |
| MCP | Typed access to individual service operations | Library and principal scope, bounded calls, policy checks, version compatibility and replay behavior |
| A2A | Submit or delegate scoped tasks, inspect progress, cancel and exchange result artifacts between agents | Authenticated identity, explicit delegation and destinations, mapping to canonical workflow state, bounded artifacts and tested interruption/recovery |

Locality, task competence, privacy and billing are separate checks. A loopback endpoint can use hosted inference; a tool-capable text model is not an audio recognizer or a calibrated evaluator. A2A and MCP connect to the same service-owned task state and policy. No adapter creates another budget ledger or grant system. Protocol research and alternatives are recorded in [agentic analysis](../../research/15-agentic-analysis.md).

## Privacy and diagnostics

Product diagnostics remain local, private and bounded. No analytics, automatic crash upload or diagnostic phone-home is part of this workflow. Explicit source acquisition and an authorized provider request are distinct purpose-bound network operations. [Private diagnostics and durable recovery research](../../research/32-private-diagnostics-and-recovery.md) links primary sources, alternatives and the canary, access, exhaustion and recovery qualification matrix.

Ordinary diagnostics contain necessary identifiers, stages, transitions, timing, resource counts and redacted failure reasons. Credentials, full prompts, hidden reasoning, full transcripts, raw media and sensitive URL components do not belong in ordinary logs. Necessary task content and selected evidence remain protected application data. Debug payload capture needs explicit local selection, finite retention and a clear deletion path. Diagnostics cannot consume capture reserves or grow without bounds. A support export is locally assembled and inspectable; external sending requires an explicit action.

## First acceptance slice

Use two already authorized sources and existing local profiles for a finite monitoring task with zero paid allowance. Persist the task, admit collection and processing through existing operations, inspect matching passages and return evidence with coverage. This can test workflow correctness before introducing open-ended discovery or hosted dispatch. Synthetic references and model-quality limitations must be identified.

Replay client exit, process death at every transition, stale proposals, duplicate calls, malformed model output, hostile retrieved instructions, provider outage, exhausted limits, deleted inputs, disk pressure, paused processing and policy revocation. Compare resumed and uninterrupted runs for effect identities, charges, evidence and outcome. Include a failed-quality case that must remain partial. Follow with scoped source discovery, archive retrieval, exact-cue playback, controlled recomputation and independently verifiable report bundles.

Long unattended runs, clean-host restore, actual platform containment, power-loss evidence and model task evaluation remain necessary for professional support claims. Exceptional reliability and long service life are design goals, not a guarantee established by tests or a label.
