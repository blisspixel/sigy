# Monitor versions and actions

Date: 2026-09-24. Status: implemented and tested on Windows x86_64. This is roadmap operation 30 in the [topic monitoring design](../design/topic-monitoring.md). Monitors do not yet schedule, match or brief.

## Decision

A monitor is an ID with immutable, numbered versions and an append-only action log.

- **Versions** hold the user's bounds: a name, the goal in the user's words, literal terms per language (any script), the sources followed now, source revisions approved in advance as candidates, saved schedules, a daily and a total audio cap, and optional recognition and translation profiles. Paid processing is off and there is no field to enable it. Every referenced source, schedule and profile must exist. The version identity is a hash of the canonical specification. Only the user creates a version, through `monitor create` or `monitor revise`; a revision must name the version the user last saw, so a concurrent change is refused rather than overwritten. An identical replay is unchanged.
- **Actions** record every proposal with its origin (user, schedule, rule or model), the version it was checked against, the decision (`applied` or `refused`), the reason, and a zero cost. A deterministic policy decides: pause and resume apply; adding a source applies only if the version lists it or approved it as a candidate; removing the last source is refused; any other request, such as raising a cap or enabling paid processing, is kept verbatim and refused as `requires-user-version`, and SQL forbids applying it. A caller-supplied action ID makes a retried proposal return the stored action, and a changed request under that ID is refused.
- **State** is derived: the followed sources are the latest version's list plus applied changes since it, and the pause state is the latest applied pause or resume. A new version resets the followed sources to its own list.

Versions and actions cannot be updated or deleted. Monitor commands are not available in the explorer yet.

## Evidence

Tests cover specification bounds (empty or oversized text, control characters, invalid languages, duplicates, caps), the policy decision for every proposal kind, digest stability, exact create and revise replay, a stale expected version, missing references, immutability, restart persistence, refusal and retention of out-of-policy and injection-style proposals, exact action replay and conflicting replay, derived pause and source state, and SQL refusal of an applied `other` action. A CLI run created a monitor with Arabic, French and English terms and recorded a refused model request, an applied candidate source and a user pause.

## Limitations

No scheduling, coverage counting, matching, findings or briefings yet. Terms are literal. There is no agent tool for monitors yet. Monitors are capped at 256 and 1,000 versions each.
