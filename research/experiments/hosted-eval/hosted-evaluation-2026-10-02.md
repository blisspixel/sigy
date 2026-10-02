# Hosted judge, translation and recognition comparison

Date: 2026-10-02. Scope: the bounded hosted comparison of increment 7 of the
[language pipeline plan](../../../docs/development/language-pipeline.md), under
the USD 20 allocation recorded before dispatch in
[progress](../../../docs/development/progress.md#spending-ledger). Tooling is the
[hosted evaluation package](README.md). This record is calibration and
screening evidence on read speech from the FLEURS calibration partition. It
qualifies no language, judge, translator or recognizer, and it involves no
human review.

## Authority and data destination

- The user approved at most USD 20 in total for this day. The tool caps itself
  at USD 18 including uncertain liabilities, and checks a per-request hard bound
  against the USD 20 ceiling.
- Only public FLEURS calibration items (CC-BY-4.0, revision
  `70bb2e84b976b7e960aa89f1c648e09c59f894dd`), constructed controls derived from
  them, recorded local outputs for those items, and one synthetic probe sentence
  per text route are sent. The [FLEURS attribution](../language-corpus/fleurs-subset-attribution.md)
  applies. No user recording, station capture, transcript of either, or holdout
  material is read or sent. The generated silence, noise and tone controls of
  the local recognition study are not FLEURS items and are not sent.
- Routing allows only the listed provider slugs, disables fallbacks, requires
  every parameter, denies data collection, and requires zero data retention on
  every route except `openai/gpt-audio-mini`, which has no ZDR endpoint in the
  snapshot and runs with data collection denied only.

## Price basis

All prices come from OpenRouter's public catalog and per-model endpoint
listings retrieved on 2026-10-02 from 14:01 UTC. Snapshots are private under
`.agents/hosted-eval/snapshots/`.

| Snapshot | SHA-256 |
| --- | --- |
| `/api/v1/models` (464 models, 762,646 bytes) | `14ecaa9a09660cf88554b5b5004948a413e272d495336d34ddac62cefa830894` |
| `/api/v1/endpoints/zdr` | `d1df76716e5b14adaa3c8924b92835780b5b35b2bc450bacc893b58108843f76` |
| `/api/v1/providers` | `762bc749316d42ddd735097833d469f855590235aa6db216e70f550542ccaebe` |
| `anthropic/claude-sonnet-5.5` endpoints | `d071d847c1bf6542bb7ffd3380a8e853fe57ed4e2061eacb502bb36a94ee8940` |
| `openai/gpt-6.1-sol` endpoints | `e3a8fec37dc19d4e404313edd2ee5f4595b2d76aab4dc523f6d6c6c97cedd731` |
| `google/gemini-3.1-pro-preview` endpoints | `40f1b7e2333c4766d2689523e66d478de4e1adcae9f8715d23dab967a1741123` |
| `google/gemini-3.8-flash` endpoints | `9c47f6dca47e57ec337cdaae08e2276c45ad341cb1309bc93991aee0a7abefa8` |
| `mistralai/mistral-medium-3-5` endpoints | `e263ca16bee6977396b43cfbf9e68de8aaa35e95bd7fa37ae8e9d8dd9fcf9702` |
| `openai/gpt-audio-mini` endpoints | `40e8b0dc2d9c10e9d71ad4b3b930093f5b70e7dcac1b7e0cddd2c6ad0655e119` |

## Routes

Models were chosen from that catalog only: three strong judges from different
vendors with structured output and a ZDR endpoint, two translators from vendors
other than at least two judges, and two audio-input models from different
vendors. Route profiles are in [`routes/`](routes/).

| Route | Model | Provider slugs | ZDR | `max_tokens` | Reasoning | Worst rate per million, prompt / completion, with 5% headroom |
| --- | --- | --- | --- | ---: | --- | --- |
| Judge | `anthropic/claude-sonnet-5.5` | `google-vertex`, `amazon-bedrock` | yes | 3072 | effort low | 4.62 / 11.55 |
| Judge | `openai/gpt-6.1-sol` | `azure` | yes | 3072 | effort low | 2.8875 / 11.55 |
| Judge | `google/gemini-3.1-pro-preview` | `google-vertex` | yes | 3072 | effort low | 3.78 / 22.68 |
| Translator | `google/gemini-3.8-flash` | `google-vertex` | yes | 2048 | effort low | 1.4175 / 7.0875 |
| Translator | `mistralai/mistral-medium-3-5` | `mistral` | yes | 1024 | effort none, temperature 0 | 1.7325 / 8.6625 |
| Recognizer | `google/gemini-3.8-flash` | `google-vertex` | yes | 2048 | effort low | 1.4175 / 7.0875 |
| Recognizer | `openai/gpt-audio-mini` | `openai` | no | 1024 | none, temperature 0 | 0.63 / 2.52 |

The worst prompt rate is the highest of the prompt, audio, cache-read and
cache-write rates of every matched endpoint, including priority and flex tiers;
the worst completion rate is the highest of completion and reasoning rates.
Web search, which every judge and Gemini route lists, is bounded at zero
searches in the reservation because no request enables search, and at one in
the hard bound. Routes with mandatory reasoning send no temperature and keep
provider-default sampling; only the two routes without reasoning request
temperature zero. Determinism is not claimed.

`max_tokens` bounds reasoning plus visible output on these providers:
OpenRouter documents that reasoning counts against `max_tokens` on most
providers, Anthropic budgets are below `max_tokens`, and Google's
[thinking documentation](https://ai.google.dev/gemini-api/docs/thinking)
(updated 2026-09-25) states that the output limit includes thought tokens. The
runner checks every reported completion count against that bound; the hard
bound uses each endpoint's output ceiling so that one wrong assumption cannot
cross the ceiling.

## Ledger allocations

Runtime ledger: `.agents/hosted-eval/ledger.jsonl`. Its first event fixes the
USD 18 software cap and USD 20 ceiling.

| Batch | Allocation (USD) | Purpose |
| --- | ---: | --- |
| `probe` | 0.500000 | One contract probe per text route, excluded from scores |
| `judge-controls` | 5.500000 | Three judges on the 126 frozen constructed controls |
| `judge-outputs` | 2.000000 | Passing judges on the existing local translations |
| `translate` | 5.000000 | Hosted translation and its judging |
| `recognize` | 5.000000 | Recognition probes and hosted recognition |
| Unallocated | 2.000000 | Headroom between the software cap and the ceiling |

## Request manifests and maximum liability

Each manifest was written before any dispatch. The per-request reservation is
the exact worst case under the bounds above, rounded up to one micro-USD.
Requests run sequentially and each settles before the next is admitted, so the
enforced maximum liability of a batch is its allocation; the sum of
reservations is what the batch would cost if every request reached every bound
at the worst listed rate.

| Plan | Manifest SHA-256 | Requests | Sum of reservations (USD) | Largest reservation (USD) |
| --- | --- | ---: | ---: | ---: |
| Probe, Claude judge | `33df93e4b3acec0803b28b4b865c6fbda9aa1866d4b20bccb79572b807075ec1` | 1 | 0.067074 | 0.067074 |
| Probe, GPT judge | `b1d94f200f7d38f9a93b45e1a78309d89a46db76531b88e52137dafe38feb92b` | 1 | 0.055129 | 0.055129 |
| Probe, Gemini judge | `b425a3895af48604f6c19b51505e442078a755fa8bd42449c85e7767d3aa5515` | 1 | 0.095464 | 0.095464 |
| Probe, Gemini translator | `788f7df4466e5c645b3806c21d99fc796c7e65f26eb0a0d3c2674028a3f25cd9` | 1 | 0.020985 | 0.020985 |
| Probe, Mistral translator | `3bf4050bc2e01f1f8017d1a1cd483ac17b3db7d6b43ededb503f1d79a3ffc6e9` | 1 | 0.016806 | 0.016806 |
| Probe, Gemini recognizer | `a0b8b3438f2936cbaf0db3380cc3757ad2c1e77e25c992ea22ed3411754dfc98` | 1 | 0.649214 | 0.649214 |
| Probe, GPT audio recognizer | `199b455b6543159054f2de33c72cef2d8a339ba47e8d7c8c20e4b57df8d336cf` | 1 | 0.083221 | 0.083221 |
| Controls, Claude judge | `f018b67e4c6717257ab5d20f51d51421cd1f9f7a4228f87cdc742323bef75809` | 126 | 8.704598 | 0.070197 |
| Controls, GPT judge | `f83092b36d6b30a213b1d7c918d4d6642e3f121318da6862d266fcea89c14c3b` | 126 | 7.104536 | 0.057081 |
| Controls, Gemini judge | `08b5799a56ef5d1e8695269ca683e3e8c5b619c35643ab2ec5acff57a66f50cc` | 126 | 12.235777 | 0.098020 |
| Gemini translation, reference text | `baf293538a60cf31366b074a63d82537c4827db84a75569d1221fe112e6e444e` | 28 | 0.594613 | 0.021517 |
| Gemini translation, recognizer text | `a4cdf1fd5925f252198da38a9f5a4f7cdd1d70bd9ae0e445d5e8282ea833513d` | 27 | 0.573025 | 0.021486 |
| Mistral translation, reference text | `96a825c2710cd7705f62a98a569ba3a9c0120a348e9466496011b3924142a195` | 28 | 0.479153 | 0.017455 |
| Mistral translation, recognizer text | `d77b12bd1ff171683f8a16f45caf41150fad16857bfbda404808c93520f8a0cb` | 27 | 0.461606 | 0.017417 |
| Gemini recognition, Swahili and Hindi | `b992046560f1d5106e1c4956a875800f107e7888226d395ad05b793e98a692af` | 8 | 6.518223 | 1.139102 |
| Gemini recognition, other six | `6c03b614ea968fc9ff096a7d21838f1a4eb2773dcbcfe694ae018ed23ccfd2e5` | 24 | 18.828914 | 1.019352 |
| GPT audio recognition, Swahili and Hindi | `08c19c9d5b25d54c483720079c76c397850b8fd45f63f2b393caf3dcb55d4622` | 8 | 0.665768 | 0.083221 |
| GPT audio recognition, other six | `35aa3e4e8561e2deeb051946513e772d0dfea55f491200ae03aa66346f05038e` | 24 | 1.997304 | 0.083221 |

Audio prompts are bounded by the request body size or the context length. That
is far above the documented audio tokenization, so the recognition
reservations are large and settle down after each request. Judge plans for the
existing local outputs are written only after the control results, for passing
judges only.

## Frozen judge contract

The hosted judges receive the same 126 controls (SHA-256
`325713cf3435b5d095a3a9058211776c76528cecaabae66d5cb0a6aa6da944cc`), the same
rubric (`1152a6a2f69173fa3ae8809f757bd82d6b6bab16b86651e3817034c426f28531`) and
output schema (`c77b04ffe36898517df197ba30f1cd3049c981c841ba1382a443a86258ccd38e`)
as the [frozen local screen](../local-mt/judge-capability-2026-09-30.md). The user
message is the local raw template's text without the local model's turn tokens:
the rubric, a blank line, `DATA JSON:` and the blinded control. The schema is
sent as a strict JSON-schema response format, the hosted analogue of the local
grammar-constrained decoding. Replies pass through the unchanged local parser
(source SHA-256 `f9a447588242ddde883f3ebb49fa6274a0def6f4544a935fe31eddd6e454a8e4`):
the trimmed reply must be exactly one JSON object with the five fields,
critical answers need unique literal quotes, and acceptable or abstained
answers need empty quotes. Any other reply, and any unattempted, ambiguous or
unsent request, is an abstention in the full denominator. The criteria are
unchanged: per language, two controls per category, at least 9 of 10 critical
detections, 0 of 8 false positives and at most a tenth of controls abstained.
Requests are independent and stateless; each judge receives the controls in a
different order derived from its route label.

## Offline baselines reproduced

These use no network and no paid request.

- The recorded local turbo recognizer outputs, rescored with the frozen scorer
  against hash-checked references, reproduce every frozen CER in the
  [32-clip recognition calibration](../local-asr/calibration-32.md): Arabic
  32/475, Mandarin 24/172, English 24/550, Spanish 17/520 on three recognized
  clips with the near-silent clip abstained, French 11/727, Hindi 143/541,
  Portuguese 20/637 and Swahili 127/614 characters. Candidate envelope
  `b454fceca998def703c58fe45fd8d8e9d692308a60b6d35783985098e9fe0c73`.
- The ported BLEU reproduces every BLEU value of the
  [translation calibration](../local-mt/calibration-32-translation.md) on the
  four recorded local runs (for example Hy-MT2 reference text 26.0 overall and
  Gemma 4 E2B reference text 32.6), and chrF++ matches the
  [offline validation](../local-mt/offline-validation-2026-09-30.md).

## Results

Dispatch status is recorded below as each batch runs.

## Limitations

- Four read-speech sentences per language and only four distinct English
  references (three for the controls). Repeated languages and mutations are not
  independent sentences. Canadian French, Navajo and Klingon are absent.
- A hosted judge passing constructed controls with a visible English reference
  shows reference-assisted discrimination, not multilingual understanding, and
  cannot qualify translation quality or a language.
- The models may have seen FLEURS or FLoRes text in training.
- One run per judge with provider-default sampling; no repeat-run stability is
  measured.
- Provider data-handling claims are OpenRouter's routing metadata, not audited.
