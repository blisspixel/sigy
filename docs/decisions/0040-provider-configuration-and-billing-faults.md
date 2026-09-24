# 0040: Provider configuration and billing faults

Date: 2026-09-24. Status: operation 26 storage and commands implemented; operation 27 fixtures pass against a fake transport on Windows x86_64. No HTTP client, provider request, or paid reservation exists. `provider_dispatch_available` stays false. Catalog v28 and local IPC v28.

## Provider facts reviewed

OpenRouter documentation, reviewed on 2026-09-24: chat completions accept no idempotency key; the generation ID arrives only in the response; `GET /api/v1/generation?id=` can return 404 before the record exists; usage and cost arrive in the final response or final stream event; HTTP 402 has a terminal budget variant and an in-flight variant with `Retry-After`, and a retry is a new billable attempt; an upstream can charge when a request errors; costs are JSON doubles; `provider.max_price` is per million tokens; `allow_fallbacks` defaults to true; routing preferences are ignored on the transcription endpoint. See [providers and cost policy](../planning/07-providers-and-cost-policy.md) and the [paid validation allocation](../development/language-pipeline.md#paid-validation-allocation).

## Configuration (operation 26)

`sigy provider route add|list|show` and `sigy provider price add|show` store and read configuration. No command sends a request, and `sigy mcp` does not expose them. The TUI explorer refuses provider operations.

A route is immutable: ID, kind (`openrouter` or `ollama`), endpoint origin, model ID, permitted upstream provider slugs, task (`translate-text`), authorized language pairs, and a secret reference. The secret reference is an environment variable name: ASCII letters, digits and underscores, not starting with a digit. Pasted key text, which contains hyphens, is refused. No secret value is read or stored, so none can reach a snapshot, export, or the catalog file. A hosted route requires an `https` origin with no path, query or credentials, at least one upstream slug, and a secret name. A local route requires an `http` loopback origin, no upstream list and no secret. `allow_fallbacks` is stored as false under a CHECK constraint. Every language pair is stored as `unvalidated`, and this schema permits no other state; promotion needs measured evidence and a later migration.

A price snapshot is immutable and belongs to one hosted route. It holds canonical exact `Rate` text for prompt, completion, request, internal reasoning, cache read, cache write, image, audio, web search, and the largest unrecognized charge, plus the RFC 3339 retrieval time, a validity window of 1 to 720 hours, and a printable ASCII source note. Prompt and completion are required; an omitted dimension is stored as zero, so the person entering it asserts that the source lists no such charge. A nonzero image, audio, web-search or unrecognized rate is stored as evidence but makes the route ineligible for a worst-case bound. A retrieval time more than five minutes in the future is refused.

Replays of identical rows are unchanged; a changed row under an existing ID is an idempotency conflict. Triggers forbid update and delete and bound counts (256 routes, 16 pairs per route, 64 snapshots per route). The migration runs in the opening transaction; a failure leaves the catalog at v27. The default global limit stays zero, so a configured route and key still cannot reserve.

## Dispatch core (operation 27)

The dispatcher, the `Transport` trait, and the attempt storage are compiled only for tests. There is no real transport and no production call path. The order of effects is:

1. Refuse a local-overload trigger before anything else. Overload never opens a paid route.
2. Answer an existing request ID as a replay without calling the transport, even after its price went stale. A changed contract under the same ID is a conflict.
3. Require a hosted route, a matching task and authorized pair, a snapshot of that route, and `retrieved <= now < valid_until`.
4. Bound prompt tokens by UTF-8 input bytes (at most 32 KiB) plus 256 template tokens, and completion tokens by the request limit (1 to 32768). Compute `Liability::worst_case` and round up.
5. Reserve that maximum against global and requested scopes and insert the attempt row in one immediate transaction. A zero limit, frozen scope, or insufficient balance reserves nothing.
6. Persist `submitted`, then call the transport once. The attempt carries the upstream list, `allow_fallbacks: false`, per-million maximum prices, the completion limit, and the secret variable name.
7. Record the outcome and generation ID once, then settle or mark uncertain. A reported cost with a valid generation ID settles, rounded up to the micro-unit; a cost above the reservation is recorded and freezes every affected scope. Missing usage looks up the generation; a 404 or failed lookup leaves the liability uncertain until a later reconcile settles it exactly once. A timeout, upstream error, or HTTP 402 keeps the full reservation as uncertain, because an upstream may already have charged. A 402 without `Retry-After` is terminal. A 402 with `Retry-After` permits one retry, only under a new attempt ID with a new reservation, after the wait, for the same route and input.

## Lifetime allowance

The user decided on 2026-09-24 that paid spend is approved once as a finite lifetime allowance, for example USD 20 in total, drawn down and never reset or refilled automatically. The existing ledger already has only this mode: a budget scope has a lifetime limit, settled and reserved totals, and no billing period, clock, or rollover. Only an explicit `sigy budget set` changes a limit, and it cannot go below commitments or thaw a frozen scope. Provider route scopes such as `provider:<route>` use the same lifetime model. A periodic budget, if ever added, must be a separate explicit opt-in and never the default. The CLI labels every limit as a lifetime limit that never resets. A fixture consumes an allowance with one settled and one uncertain attempt, then shows that further dispatch is refused, with no reservation and no send, after a restart and with fresh price snapshots dated a day, a month and a year later.

OpenRouter's key-level `limit` with `limit_reset: null` should be configured on the dedicated key as a second line of defense behind the Sigy ledger, never as the primary control. Sigy does not read or set that limit, and its in-flight and billing semantics are not verified here.

## Recovery

A process loss after sending leaves the request `submitted`; `recover_submitted` turns it into an uncertain liability on reopen, and replay does not resend. The ledger audit and a provider audit (outcome before settlement, evidence equals generation ID, snapshot route matches) pass after every fixture.

## Limitations and evidence needed

The prompt bound assumes each tokenizer token covers at least one content byte and that the chat template adds at most 256 tokens; neither is proven for any specific model. A reported cost without a usable generation ID is not settled and stays uncertain until manual reconciliation, which does not exist yet. Outcome recording and the ledger transition are separate transactions; a crash between them leaves the request submitted, which recovery treats as uncertain. The transport trait is synchronous and fake; a real client, its timeout and cancellation behavior, the provider-side meaning of `max_price` as a double, 402 classification from real responses, and bring-your-own-key billing need contract tests before any live use. Route destinations are not yet qualified, a secret name is not checked for presence, and no language pair can be validated. Enabling dispatch requires a real transport, a supervisor that runs `recover_submitted` before admission, a recorded reservation in the work ledger, and a nonzero configured limit. This does not complete [operation 28](../../ROADMAP.md#build-order).
