# Model providers and cost controls

Reviewed: 2026-09-20. Status: official API research; no paid calls or account changes.

## Local and network runtimes

Ollama documents generation, structured outputs, and inspection of loaded models. These are useful building blocks for provider capability discovery and typed task output. Structured JSON still requires application validation and evidence checking. [Generation](https://docs.ollama.com/api/generate), [structured outputs](https://docs.ollama.com/capabilities/structured-outputs), [running models](https://docs.ollama.com/api/ps).

Ollama also documents cloud functionality and a local-only configuration. Therefore, a loopback connection is not sufficient evidence that a selected route performs inference locally. Sigy needs explicit model destination policy and must not alter global runtime settings silently. [Ollama FAQ](https://docs.ollama.com/faq#how-do-i-disable-ollama-cloud-features).

The llama.cpp server is another researched integration candidate with an HTTP serving interface. Sigy's internal task types should not be tied to a runtime-specific SDK or response structure. [Server documentation](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md).

## OpenRouter findings

| Capability | Documented evidence | Design implication |
| --- | --- | --- |
| Inspect current key | Limit, remaining limit, reset interval, usage, and BYOK fields are exposed | Inspect and report provider constraints alongside the local ledger; [current-key API](https://openrouter.ai/docs/api/api-reference/api-keys/get-current-api-key) |
| Usage accounting | Usage includes costs and token detail; streaming usage arrives at the end | Persist final usage, handle missing terminal events, and avoid treating partial output as final billing; [usage accounting](https://openrouter.ai/docs/cookbook/administration/usage-accounting) |
| Generation lookup | Generation metadata can be retrieved by ID | Retain IDs for reconciliation after interruptions; [generation API](https://openrouter.ai/docs/api/api-reference/generations/get-generation) |
| Model catalog | Model properties and pricing are available | Retain dated pricing snapshots and explicit units; [model catalog](https://openrouter.ai/docs/api/api-reference/models/get-models) |
| Routing restrictions | Provider/model routing can constrain price and allowed routes | Apply approved routes and price bounds on every relevant request; [official routing explanation](https://openrouter.ai/blog/insights/model-routing/) |
| Key/member guardrails | Budget and allowlist controls are documented with layered scopes | Detect availability and semantics for the actual account; [guardrail documentation](https://openrouter.ai/docs/guides/features/guardrails/overview) |

Workspace budgets are documented as an Enterprise feature. Their checks precede routing, while already dispatched requests finish; the documentation explicitly allows actual spend to exceed the threshold. Workspace BYOK inclusion is optional and uses a specified accounting basis. Sigy must not equate this setting with a universal strict cap on all upstream bills. [Workspace budgets](https://openrouter.ai/docs/guides/features/workspaces/workspace-budgets).

## Proposed architecture

Support capability-based provider adapters for on-device, loopback, LAN, and hosted processing. Track actual destination and billing route separately. Local ASR with remote text translation is a possible explicitly configured profile; sending raw audio requires separate task capability and policy.

Use a central, transactional budget ledger with pre-submission reservations, request IDs, priced bounds, and reconciliation. Add dedicated provider-side key limits where available. Unknown charges keep their reserved liability until resolved. The complete policy is in [Providers and cost policy](../docs/planning/07-providers-and-cost-policy.md).

A model cannot authorize extra spending or rewrite its own budget. Retries, automated discovery, summaries, embeddings, and external identification services all participate in the same admission mechanism.

## Required validation

Verify caps against the actual endpoint and model, including reasoning tokens, cache-write fees, multimodal pricing, output limits, provider fallbacks, tools, and long-context tiers. Test the distinction between an HTTP transport retry before submission and a retry after uncertain acceptance.

Begin with deterministic provider fixtures. Later, an explicitly bounded live test checks final usage against provider billing and exercises interrupted streaming. Pricing or policy fields absent from a provider are unsupported capabilities, not permission to assume a zero cost.

## Near future

Provider APIs and routing behavior are fast-moving. The design should tolerate new billing dimensions by rejecting unsupported strict-budget configurations until the adapter can bound them. Keep provider selection, model versions, prices, and effective routes in each result's provenance.

Do not build the first release around organizational features unavailable to ordinary accounts. Do not depend on a provider's future spending-control roadmap to meet the product's current requirements.
