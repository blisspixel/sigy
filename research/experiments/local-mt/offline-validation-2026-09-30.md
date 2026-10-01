# Bounded offline translation validation

Reviewed and replayed 2026-09-30 on the Windows development host. Scope: Rust
scoring of the already recorded [32-clip translation calibration](calibration-32-translation.md)
and preparation of offline critical-error judge calibration. No new inference,
download, hosted request, paid reservation, or holdout reference access occurred.
External spend remains USD 0.

## Implemented evaluation boundary

The [offline scorer](../language-scorer/README.md#translation-screening) now
checks an immutable artifact digest before parsing a bounded JSON envelope.
Each of the 28 non-English calibration inputs must have a published original
and parallel English reference matching the frozen selection's transcription
hashes. Candidate inputs and outcomes remain separate from references.
Reference-text translation must have used that original source string;
recognizer-text translation declares its recognizer profile and artifact
identity and independently hashes each actual translator input.

chrF2++ follows the [reviewed pinned upstream implementation](https://github.com/mjpost/sacrebleu/blob/c596d9d2072a8f84200574a7a5c56c618e8d37e8/sacrebleu/metrics/chrf.py).
Its [character n-gram helper](https://github.com/mjpost/sacrebleu/blob/c596d9d2072a8f84200574a7a5c56c618e8d37e8/sacrebleu/metrics/helpers.py)
fixes whitespace removal, and its word procedure splits one trailing ASCII
punctuation character before considering a leading character. This Rust
implementation preserves case and Unicode forms, uses six character and two
word orders, beta 2 and effective-order averaging, and retains exact n-gram
counts beside derived floating-point scores. It adds no dependencies.

The alternatives were retaining the disposable unbounded scorer, adding a
Python evaluation dependency, or relying only on heuristic entity/quantity/
negation flags. The first lacked the existing scorer's frozen identity and
resource boundaries, the second conflicts with the project language contract,
and the flags already missed severe meaning errors. chrF++ supplies reproducible
screening while the separate judge-control tool makes its semantic limitation
explicit. BLEU remains in the historical study; this increment implements only
the roadmap's primary chrF++ metric.

## Existing calibration replay

A local Rust adapter converted evaluator-only `items.json` and
`translations.jsonl` into strict envelopes under
`.agents/language-evaluation/mt/replay/artifacts/`. It copied existing text and
outcomes; it did not generate answers. The prior Spanish `no-input` outcome
becomes an explicit abstention with no translated text. Each of the four runs
keeps all 28 selected inputs.

| Existing run | Translated clips | All 28 chrF++ | Translated-only chrF++ | Distinct English references |
| --- | ---: | ---: | ---: | ---: |
| Hy-MT2, reference text | 28 | 54.7018671718787 | 54.7018671718787 | 4 |
| Hy-MT2, recognizer text | 27 | 45.2200644218705 | 46.5363088157684 | 4 |
| Gemma 4 E2B, reference text | 28 | 59.6662808925469 | 59.6662808925469 | 4 |
| Gemma 4 E2B, recognizer text | 27 | 49.2004671275773 | 50.6559608935475 | 4 |

All 28 original and English reference pairs passed their frozen hashes. The
32 translated-only per-language and overall scores (four runs times eight
aggregates) equal the stored historical scorer values at stored floating-point
precision. The historical study separately compared its scorer with the pinned
upstream implementation. This replay confirms implementation agreement; it
does not rerun that implementation or improve the sample's independence.

The all-clips difference on recognizer text exposes the missing Spanish output
in the reference denominator. Read the conditional result with that coverage.
Four repeated parallel sentences cannot rank models, qualify a language, or
establish broadcast behavior. Canadian French, Navajo and Klingon remain absent.

Provenance receipts:

| Artifact | SHA-256 |
| --- | --- |
| Existing `items.json` | `6c9ed6f3299c0089f80d798df785eeb29c4a1ec485bf1c7305622e01dd11447d` |
| Existing `translations.jsonl` | `d655a569ac0ab86aecf1cb328ad4182c2c65d4e24cbe1853be3e0935e8990fe5` |
| Rust replay adapter source | `c89e558af561598dcad344970e7aae9c903980f11122a9a0ad0d892bed1ba972` |
| Strict reference envelope | `533b5d443e687a4e03c8b0adb98de198f3c159a9c5f44d60de0168ebba2f8aed` |
| Hy-MT2 reference candidate envelope | `32660c66b06940f7182f7391c4fed6599661b7750f55c6ff9fdabc9865beb743` |
| Hy-MT2 recognizer candidate envelope | `73fc5508e9ea924125aa88f0cf53f26d9c014247215b7d0f93772b328b4c72f0` |
| Gemma reference candidate envelope | `81ebfffc93333f5ae3434b18e7ae7912c50c63bf7f1613f4aee492ae0c2a5b16` |
| Gemma recognizer candidate envelope | `b444fdabc55b0f426b02112eff0411d029855faf7c126862f1659128c7be6157` |

Replay profile digests identify the model name and recorded output artifact,
not a newly verified executable/model/template closure. Native profile
identities remain those in the original study. The recognizer artifact digest
identifies the recorded calibration item collection; it is not a service
transcript revision or independent authentication. The scorer reports those
identities as declarations and preserves their actual text hashes.

## Critical-error calibration preparation

The [judge-inputs and judge-score commands](../language-scorer/README.md#offline-judge-control-calibration)
run no models. Calibration controls must carry verified published original and
English references, opaque IDs and documented unchanged, formatting or
deliberate-mutation lineage. Blinded judge inputs omit expected categories,
criteria and mutation metadata. Judgment artifacts require complete outcomes,
matching rubric and profile identities, and literal original/reference ranges
for a critical judgment.

Reports retain exact sensitivity, false-positive and abstention counts by
language and category, including missing coverage. Frozen rational criteria
must pass separately for each language. Critical abstentions stay in the
sensitivity denominator; an independent ceiling stops an all-abstain judge
from passing. A supplied quotation must match a valid UTF-8 range. Neither
literal membership nor a verified string mutation proves semantic correctness.
Critical categories and severity remain declared experiment labels.

This artifact replay did not run a judge. The subsequent
[frozen reference-assisted capability screen](judge-capability-2026-09-30.md)
ran 126 controls and failed the declared criteria in every language group.
It is not a qualified judge. Before using another profile, choose
independent model families with task-specific capabilities, hash their assets
and rubric, justify and freeze thresholds, prepare sufficient per-language
controls with published reference basis, vary answer order, and test deliberate
negation/entity/quantity/omission changes, fluent unrelated output and untrusted
instructions. Keep disagreements unresolved. Passing a deliberately constructed
control set still does not qualify held-out or broadcast translation.

## Verification and next evidence

The standalone package passed 45 tests, formatting, warnings-denied Clippy,
offline locked build, and a cached advisory check. New tests cover strict
source/English pairing, identity conflicts, missing and cross-partition clips,
explicit absent output, exact maximum-size metric counts, declared-control
fractions, blinded inputs, false positives, abstentions, and fabricated or
split-character quotes. Synthetic boundary tests make no language-quality
claim. The cached advisory check does not prove current advisory freshness.

The next quality increment is a separately frozen capability profile and
genuinely independent judge runs with fresh controls and broader published
calibration references before any holdout is opened. Translation and
speech-language detection remain
unqualified. The tool supports holdout scoring only after a profile is frozen;
this run did not inspect held-out reference strings.
