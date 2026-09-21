# Providers and cost policy

Status: proposed operational design implementing confirmed requirements C-04, C-11, C-14, and C-15. Research date: 2026-09-20. Supporting evidence is in [Model providers and costs](../../research/04-model-providers-and-costs.md).

## 1. Processing locations

| Location | Example | Configuration and enforcement |
| --- | --- | --- |
| On-device library or worker | A local speech runtime | Model asset, process/resource policy, no network inference assumed |
| Loopback service | Ollama on this computer | Endpoint, model, capability check, effective destination policy |
| User-managed network host | Ollama or another runtime on a LAN server | Explicit host, authentication/TLS as applicable, latency and outage handling |
| Hosted provider | OpenRouter or another configured API | Explicit provider/model authorization, secret, routing policy, enforced budgets |

Location, ownership, privacy behavior, and billing are separate properties. A local endpoint can proxy hosted inference. A LAN service can have its own usage charges. Configuration describes these properties rather than inferring them solely from the URL.

Ollama and OpenRouter are required integration targets, not dependencies for basic radio use. Additional adapters follow a capability contract. Models can be assigned separately to transcription, translation, embeddings, extraction, briefings, and planning.

Local language detection, transcription, and queued analysis must support useful operation with a paid budget of zero. Optional classification is a separate capability and stage allocation. Jev through OpenRouter is a candidate dedicated decision endpoint, not an assumed chat-compatible model. Its billing, retries, version identities, routing, and reconciliation need qualification before strict admission. [Decision-model research](../../research/16-decision-models-and-classifiers.md).

## 2. Authorization model

Paid processing is disabled until the user enables a provider and sets finite limits. Adding credentials alone does not authorize every monitor to use them.

A saved policy defines allowed tasks, models, providers, billing routes, data types, source collections, fallback behavior, and budgets. Activating a monitor authorizes repeated work inside that policy. Requests inside the policy do not require repeated prompts. Widening the policy is a user operation.

The service must never purchase credits, enable auto-recharge, raise a provider-side limit, switch to an unapproved paid model, or consume another configured provider's budget merely to keep a task running.

The global paid-processing stop control prevents new submissions immediately. Already accepted remote work may remain billable and must stay in the ledger until reconciled.

## 3. Budget hierarchy

Enforce every applicable limit, not just the closest one:

- Installation-level daily/monthly and optional lifetime budget.
- Provider/account and dedicated API-key budget.
- Monitor budget for each period and optional total run budget.
- Task/stage allocation, such as discovery, translation, and briefing.
- Per-request maximum liability, output and reasoning allowance, retry count, and tool-call count.
- Concurrency and request-rate caps, independent of monetary limits.

Currencies and reset windows are explicit. Preserve provider billing currency. Display currency conversion only as an estimate with its timestamp. Use exact decimal or appropriately scaled integer amounts for enforcement, never binary floating point.

A user can set alerts below the hard limit, but alerts do not replace admission control. Local compute remains subject to CPU, memory, storage, and concurrency limits even when provider charges are zero.

## 4. Reservation before submission

Maintain an append-only accounting history with current transactional balances. A single authoritative service coordinates local reservations.

For each applicable budget:

```text
available = configured_limit - settled_spend - outstanding_reservations
accept only if maximum_request_liability <= available
```

Unresolved submitted requests retain their reservations. They must not be subtracted twice as both a reservation and a separate uncertainty balance.

Request admission is one atomic transaction across all applicable budget counters and the request record. Concurrent workers cannot each observe the same unreserved balance. Persist the reservation before transmission; persist the attempt before an uncertain network boundary.

Illustrative arithmetic, not provider pricing: with a $5.00 limit, $3.20 settled, and $1.00 reserved, only $0.80 remains. Two simultaneous requests each requiring $0.60 cannot both be admitted.

### Maximum liability

Calculate the upper bound from verified billable dimensions and enforceable request limits: bounded input tokens, maximum billed output including reasoning where applicable, audio duration/tokens, per-request charges, cache-write charges, tools/search, and provider fees where applicable.

Use the allowed routing set's worst permitted applicable price. A cheap expected provider is not the upper bound when fallback allows a more expensive provider. Do not assume cache hits or estimate input token count using an average characters-per-token ratio when enforcing a hard cap.

If exact tokenization is unavailable, require a proven conservative bound or a provider-enforced total limit. If a billing dimension cannot be bounded or an endpoint ignores required limits, strict-budget autonomous use of that configuration is unavailable. Explain the unsupported capability and retain the work locally.

Record the pricing snapshot, its unit, retrieval time, model and provider IDs, routing settings, tokenizer/bounding method, and safety margin. Catalog prices can use different units from routing constraints; conversion is explicit and tested.

## 5. Price freshness and routing

Before accepting paid work, ensure the price snapshot is within its configured validity window and that provider capabilities still support the bound. Unknown or stale pricing stops new paid submissions under strict mode.

Restrict model and provider routing to the approved set, with supported provider-side maximum-price fields. Parameter support is validated; unsupported limits cannot be silently ignored. Automatic model routing is off unless every reachable route satisfies policy and its worst-case liability can be reserved.

Price decreases do not require budget expansion. A price increase invalidates an old estimate and triggers fresh admission under the existing cap. Inability to reserve pauses that stage; it does not enlarge the cap.

No specific model price is hard-coded into the design. Current prices are retrieved and retained as dated evidence when selecting a paid profile.

## 6. Submission, completion, and uncertainty

| Event | Ledger behavior | Job behavior |
| --- | --- | --- |
| Reserved, never transmitted | Reservation can be released once non-submission is established | Safe to retry with fresh admission |
| Request accepted, result received | Reconcile authoritative usage against reservation | Commit output with request/model provenance |
| Client stops watching | No accounting change | Remote work and durable job follow their own lifecycle |
| Timeout after possible submission | Keep maximum reservation; mark outcome unknown | Do not blindly retry |
| Stream ends without final usage | Keep reservation; reconcile by provider request/generation ID | Show output and billing completeness separately |
| Retry is authorized | New reservation for a potentially billable attempt | Bound cumulative attempts and costs |
| Provider refuses for budget/rate | Classify correctly; reconcile any possible charge | Wait or stop according to saved policy |
| Actual charge exceeds reserved bound | Record actual amount, freeze new affected paid work, flag policy breach | Require investigation before resuming that profile |
| Service crashes | Recover durable reservations and attempt records | Reconcile before resubmission |

Provider idempotency support must be verified per endpoint. Internal idempotency only prevents duplicate local result publication; it cannot guarantee a remote service executed or billed once.

Unknown charges remain visible and conservatively reserved until authoritative evidence or a documented manual reconciliation resolves them. A restart, deletion of a monitor, or a new budget period must not erase liability.

Provider-managed continuous jobs need an enforceable expiry, finite total liability, or provider-side spending bound that remains effective if Sigy is offline. A local stop timer cannot bound a remote job that keeps billing after a host failure. Prefer bounded clip requests for music identification until a continuous-service contract passes this requirement. Creating paid subscriptions or enabling automatic renewal is a separate explicit action, never an incidental monitor operation.

A budget period uses an explicit time zone and rollover policy. In-flight commitments remain accounted for across the boundary. Clock rollback, forward jumps, and daylight-saving changes cannot renew a budget twice or free unresolved reservations.

## 7. Independent provider-side controls

Recommend a dedicated limited key for Sigy. Inspect available key limits and reset behavior without requiring a powerful account-management key for routine inference.

For OpenRouter, research supports key-limit inspection, routing price restrictions, usage reporting, and generation lookup. Provider-side budget controls are a second boundary. Their in-flight and billing semantics still need contract tests; an account balance is not the Sigy budget.

Workspace budgets are documented as Enterprise-only, and already dispatched requests can take actual spend above the threshold. They are optional additional controls, not the first release's basis for a strict cap. [Workspace-budget documentation](https://openrouter.ai/docs/guides/features/workspaces/workspace-budgets).

Bring-your-own-key routes can create charges outside the credit balance. Track those routes separately, inspect whether limits include them, and use upstream controls where available. Do not count the same charge twice when reconciling provider fees and upstream usage.

Other applications using a shared key can consume its allowance. A dedicated key reduces ambiguity. Sigy's ledger accounts for its own work, while provider limits constrain the credential's broader usage.

Strict mode's acceptance claim is bounded authorized submissions under validated billing contracts. Provider-side enforcement, explicit maximum request bounds, and conservative reconciliation are necessary to substantiate a no-surprise-spend experience. The interface must not claim cancellation makes an already submitted request free.

## 8. User experience

Before activation, show task-specific destination and limits: which processing stays local, which text/audio leaves the host, allowed models, maximum rate, per-run and recurring caps, and what happens at the cap.

During a run, show settled cost, reserved maximum, unresolved charges, remaining budget, current estimated burn rate, and projected time to the limit. Estimates are labeled and never drive enforcement alone.

At the cap, the affected processing stage pauses. Capture may continue only within its separately authorized retention and storage budget. The user can use an already approved local alternative, wait for the next period if authorized, or revise the policy.

CLI and TUI expose the same inspectable ledger. Exports redact secrets and preserve pricing/usage evidence needed to explain costs. Provider test commands distinguish free connectivity checks from paid sample inference.

## 9. Cost-control verification

Required tests include simultaneous requests racing for the last balance, process death at every submission boundary, missing usage events, duplicated completion events, price changes, inconsistent billing units, retry storms, provider fallback, reasoning tokens, cache writes, external tool fees, BYOK, shared-key consumption, period rollover, and clock changes.

No live paid experiment occurs during documentation research. Later tests start with deterministic provider fixtures. Live billing reconciliation requires a separately bounded, deliberately enabled test budget and records actual results.
