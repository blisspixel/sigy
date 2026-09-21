# Universal signal interpretation

Updated: 2026-09-20. Status: core product direction and implementation contract. Sigy aims to turn unfamiliar observations into understandable, inspectable meaning across languages and representations. Canadian French, Navajo, and Klingon are minimum named acceptance cases, not a limit on the language ambition. Mathematical and symbolic content belongs in the same interpretation architecture.

## User journey

A user selects a live source, retained interval, file, or supplied text and asks what it means. Sigy identifies plausible representations, uses appropriate decoders and models, checks what can be verified, and presents an explanation linked to the original. The user need not assemble the pipeline manually. Advanced inspection shows the intermediate forms and why a route was selected.

The input might be mixed-language speech, sung material, timed symbols, a numerical sequence, a packet payload, an equation in a document, or telemetry with missing units. An observation can contain several of these. An unknown input must remain eligible for investigation instead of falling out of a fixed language menu.

## One extensible interpretation path

| Stage | Responsibility | Durable output |
| --- | --- | --- |
| Acquire and preserve | Capture authorized material with finite resources | Original artifact, source configuration, timing, gaps and retention references |
| Characterize | Detect candidate media, framing, script, language, symbol system and structure | Observations with method, granularity and uncertainty |
| Decode / recognize | Apply qualified signal decoders, ASR, OCR or structured parsers | Reproducible representations tied to original ranges |
| Interpret / translate | Explain meaning, language, mathematics, units or domain context | Interpretations, assumptions, translations and evidence references |
| Verify / resolve | Check syntax, checksums, mathematical constraints, references or supplied-key authentication | Check results, contradictions and unresolved alternatives |
| Present and accept corrections | Show useful meaning with provenance; accept scoped human context | New revisions, corrected routes and replayable evidence |

These stages reuse typed artifact, transform, provider, scheduling, storage and cost contracts. Format identifiers, natural-language tags, and mathematical/symbolic representations are distinct types. A numerical stream does not need to masquerade as English text to enter the system.

Route selection combines validated capabilities with model judgment about unfamiliar meaning. Models can propose decoder/model/tool combinations; the service admits only allowlisted operations inside source, resource, time, and spending policy. Deterministic parsers establish syntax and exact properties. They do not establish semantic meaning by keyword matching.

## Mathematics and unfamiliar structure

Preserve the original notation and construct a typed expression or structured hypothesis with explicit assumptions. Distinguish `x` as a variable from multiplication, a numeral from a letter, decimal conventions, units, domains, and missing context. A renderer or OCR result is not a proven interpretation.

For a recognized expression, a bounded calculator, symbolic engine, unit checker, or solver can verify applicable claims. A solver proves properties of the supplied formalization, not that the formalization captures the source's intended meaning. Model-produced expressions never execute as general-purpose code or shell commands.

For an unfamiliar sequence or symbol system, keep competing hypotheses and identify which further observations would distinguish them. A finite sequence can fit many rules. Do not convert an attractive pattern into certainty. A supplied legend, parallel text, protocol description, or corrected example can make a previously unknown source interpretable; preserve that context and its origin.

Known encodings and historical cipher experiments retain the existing modern-cryptography boundary. Authenticated decryption uses supplied keys and validated protocols. Recognition of encrypted data does not imply recovering its plaintext without a key.

## Minimum evidence, expanding reach

Acceptance includes Canadian French regional speech, Navajo speech/text, Klingon speech/text, mixed-language intervals, mathematical notation, typed numerical/telemetry examples, unfamiliar symbols, and deliberately ambiguous inputs. These are requirements to solve and qualify, not labels to add to a menu. A single model's missing vocabulary is a routing, model-adaptation, or data-research problem. It must not silently redefine the product's scope.

Maintain local-first paths and useful zero-metered-cost processing. Broader coverage may need several recognizers, translation models, terminology resources, and carefully licensed adaptation data. All attempts remain bounded and preserve the original for improved reprocessing. Paid capability is optional and explicitly admitted through the same exact ledger.

Report what was directly decoded, mechanically checked, model-inferred, human-supplied, and unresolved. Confidence must have a task-specific basis; do not average unrelated scores into a fictitious universal reliability number. If a route is inadequate, retain the unmet requirement and next experiment instead of claiming completion or deferring it as a cosmetic feature.

The complete first radio release retains its explorer, recording, multilingual translation and monitoring commitments. The interpretation architecture belongs in that foundation. Physical radios retain their existing hardware roadmap; file/text/retained-artifact interpretation can develop without devices. See [research](../../research/24-universal-interpretation.md), [language coverage](languages.md), and the [assurance register](../planning/04-assurance-and-validation.md).
