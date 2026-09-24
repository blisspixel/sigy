# 32-clip calibration translation check

Run: 2026-09-24 on the development host (Windows 11 x86_64, Ryzen 7 7840U, about 64 GiB RAM). Scope: English translation of the 28 non-English sentences of the frozen FLEURS calibration partition (4 train sentences each for `ar_eg`, `cmn_hans_cn`, `es_419`, `fr_fr`, `hi_in`, `pt_br` and `sw_ke`) by two local models, once from the published FLEURS transcript and once from the turbo recognizer output of the [32-clip recognition calibration](../local-asr/calibration-32.md). Scores compare each output with the parallel FLEURS English sentence. This is calibration evidence for a research harness. It is not a quality claim, a language-support claim, a human review, or a qualification of the [local translation worker](../../../docs/decisions/0041-local-translation-worker.md). Holdout (test) references were not opened.

## Inputs and assets

| Object | Identity |
| --- | --- |
| Runtime | llama.cpp release `b11146` (commit `7fe450e19305b828c199d602c23a8337aaa1f03b`), `llama-b11146-bin-win-cpu-x64.zip`, 18,560,055 bytes, SHA-256 `14cf1303ca9ac3abd94816850532f9f9a69ac66fbaca3776fc6f9061c2fac1d1`, equal to the GitHub release asset digest; `llama-completion.exe` SHA-256 `3427f711f8d20ddb4f141cd89f3ef0c4351e389fa5d54af1d503ff778560518a` |
| Model A | `tencent/Hy-MT2-1.8B-GGUF` revision `a0c709d9fac510f2c807aa3af52872340dc37a4a`, `Hy-MT2-1.8B-Q4_K_M.gguf`, 1,133,080,448 bytes, SHA-256 `dc5f44fcf1fa496ee7ad725982c0c8c553a4de00259b53af84c4b89fb0c06699`, Apache 2.0 |
| Model B | `ggml-org/gemma-4-E2B-it-GGUF` revision `b4243c156154b6dca9324415f8c7ccc098b4aed1`, `gemma-4-E2B-it-Q4_0.gguf`, 2,841,481,184 bytes, SHA-256 `8e30dff3ac4c8434c49a7036fa15564bdbb6044e42bf04550bf1a096ad7e6a52`, declared `apache-2.0` in the repository card and the GGUF metadata |
| Corpus | `google/fleurs` revision `70bb2e84b976b7e960aa89f1c648e09c59f894dd`, CC-BY-4.0, train split only |
| Metric reference | `mjpost/sacrebleu` commit `c596d9d2072a8f84200574a7a5c56c618e8d37e8`, source archive SHA-256 `f04354e671358b0a5f8a8cb061cdb8a37b1ac1e78dfbd77d0722e25baed7d8a7` |

Both model files were already in the workspace. Each was re-hashed before the run, and each SHA-256 equals the publisher LFS object ID at the revision above. No model bytes were downloaded for this run. The only new transfers were the sacrebleu source archive (1,869,745 bytes), nine raw sacrebleu source and test files (83,739 bytes) and repository metadata queries.

Each source transcript's SHA-256 equals `raw_transcription_sha256`, and each English reference's SHA-256 equals `parallel_english_reference_sha256`, in the frozen screening manifest, for all 28 items. The English reference is the `en_us` train row with the same FLEURS sentence ID. The calibration partition uses four parallel sentence groups (IDs 264, 773, 831 and 1087), so the 28 items share only four distinct English references. Reference 264 retains the FLEURS editorial bracket in `con[c]essions`; it was not edited.

## Method

Each translation ran one `llama-completion` process, one at a time, with `OMP_WAIT_POLICY=PASSIVE`, standard input from the null device, the prompt in a private file passed with `-f`, and `--offline --jinja -st --no-display-prompt --no-warmup -n 256 -c 2048 --temp 0 -s 0 -t 4`. A 180 s wall deadline was enforced with `taskkill /F /T`. Output cleanup removed terminal escape sequences, a trailing `[end of text]` and surrounding whitespace, nothing else. Unlike the product worker, this harness did not use a Job Object, a memory ceiling or a cleared environment, and memory was not measured.

Both models received the same instruction: `Translate the following text into English. Note that you should only output the translated result without any additional explanation:` followed by a blank line and the source text.

- **Hy-MT2** used conversation mode with its embedded chat template.
- **Gemma 4 E2B** needed a different route. In conversation mode, this build rendered a system turn containing `<|think|>` even with `--reasoning off` (confirmed with `--verbose-prompt`). The model then wrote a visible thought channel and reached the 180 s deadline on a one-sentence probe. Prefilling an empty thought channel made the model copy the French source instead of translating it. The route used was `-no-cnv` with a raw prompt equal to the model's own template output with thinking disabled and no system turn: `<|turn>user\n{instruction and text}<turn|>\n<|turn>model\n`, with BOS added by the tokenizer (confirmed with `llama-tokenize`). None of the 56 Gemma outputs contained channel or turn tokens.

The ASR input is the turbo + VAD text from the recognition calibration. The near-silent `es_419` clip 18118144988866964999 produced no text, so it has no ASR translation and is excluded from the ASR rows; `es_419` ASR rows therefore cover three sentences. The `hi_in` clip 17512382642704834946, which the recognizer wrote in Perso-Arabic script and labeled `ur`, is included as recognized.

Swahili is not a declared Hy-MT2 language. It was run anyway and is reported separately. Summary rows show the six declared languages and all seven.

### Metrics and scorer validation

A new binary, `mtscore`, in the disposable `scorer-probe` crate implements chrF++ and BLEU as sacrebleu 2.x defines them. chrF++ is chrF2++: character n-grams 1 to 6 with whitespace removed, word n-grams 1 and 2 after sacrebleu's single leading or trailing punctuation split, beta 2, case kept, effective-order averaging (sacrebleu's default, not epsilon smoothing), and the best reference per segment. Corpus BLEU uses the 13a tokenizer, exponential smoothing and no effective order. Sentence BLEU uses 13a, exponential smoothing and effective order, as `sacrebleu.sentence_bleu` does. Corpus scores pool sufficient statistics over the sentences in the row, as sacrebleu does. Source SHA-256: `c3425513996c4c20f362e4d185f637a6d34cac497dde6a6895161bc704a1b1a3`.

Validation, in two independent ways:

1. Unit tests (9 passing, warnings-denied Clippy clean for the binary) reproduce the published expected values in sacrebleu's own test suite and README at the pinned commit: all 13 epsilon-smoothed chrF cases (for example 63.361730 and 64.1302698), the 7 effective-order chrF cases, the 2 whitespace-kept chrF cases, the 3 Czech sentence-level chrF cases (39.14078509, 31.22557079, 57.15704367), the README multi-reference corpus BLEU 48.530827 (13a), 49.1919566 (no tokenizer), 48.530827 without smoothing, the variable-reference 29.44, and chrF2 59.73; empty hypotheses giving 0.0; the `raw_corpus_bleu` cases including the n-gram statistics `[4, 2, 1, 0] / [6, 5, 4, 3]`, 0.1555722182 and 0.8375922397; and the Romanian sentence-BLEU cases for exp (8.493), none, floor 0.1 and 0.5, add-k 1 and 2, and no tokenizer (7.347). sacrebleu publishes no small chrF++ value, so one was computed by hand: `the cat` against `the dog` gives 20.625 (eight effective orders, precision equal to recall, mean 1.65 / 8). The 13a tokenizer strings in the tests were checked against sacrebleu's `Tokenizer13a`.
2. The pinned sacrebleu source itself was run on this study's actual outputs, without installation. Display and file-locking imports that chrF and 13a BLEU do not use were stubbed. Over every sentence-level and corpus-level value in the tables below, the largest absolute difference from `mtscore` was 2.1e-14. sacrebleu also gave 20.62 for the hand example.

Critical-error screen. For each output, three heuristic candidate flags were computed against the English reference: a **number** flag when a digit sequence in the reference (thousands separators removed) is absent from the output; a **negation** flag when `not`, `never`, `no` or an `n't` contraction appears in exactly one of the two; and an **entity** flag when a capitalized reference token that is not sentence-initial and does not follow an opening quote or parenthesis is absent from the output, case-insensitively. These are candidate flags, not verdicts.

## Results

chrF++ and BLEU, per language, pooled over that language's sentences. `n` is 4 except `es_419` ASR (3).

| Language | Hy-MT2 ref chrF++ | Hy-MT2 ref BLEU | Hy-MT2 ASR chrF++ | Hy-MT2 ASR BLEU | Gemma ref chrF++ | Gemma ref BLEU | Gemma ASR chrF++ | Gemma ASR BLEU |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| ar_eg | 54.6 | 21.9 | 49.0 | 18.4 | 62.9 | 38.8 | 56.5 | 35.2 |
| cmn_hans_cn | 49.9 | 17.2 | 45.6 | 14.6 | 48.5 | 18.1 | 42.5 | 14.4 |
| es_419 | 59.5 | 31.1 | 57.8 | 26.0 | 56.6 | 26.9 | 55.4 | 27.0 |
| fr_fr | 57.7 | 31.7 | 54.5 | 25.9 | 63.3 | 33.5 | 59.9 | 32.8 |
| hi_in | 58.4 | 26.9 | 38.6 | 11.4 | 61.9 | 28.5 | 39.3 | 13.0 |
| pt_br | 64.6 | 38.8 | 57.3 | 28.4 | 69.8 | 46.6 | 61.1 | 35.0 |
| Six declared languages | 57.4 | 28.2 | 50.2 | 21.2 | 60.5 | 33.1 | 52.4 | 27.1 |
| sw_ke (undeclared for Hy-MT2) | 38.6 | 13.8 | 27.0 | 1.4 | 54.7 | 29.8 | 40.7 | 11.8 |
| All seven | 54.7 | 26.0 | 46.5 | 17.5 | 59.7 | 32.6 | 50.7 | 25.1 |

On the 27 sentences that have an ASR input, the reference-text rows are 54.8 chrF++ / 26.4 BLEU for Hy-MT2 and 60.1 / 33.2 for Gemma, so the reference-to-ASR drop on a matched set is 8.3 chrF++ for Hy-MT2 and 9.4 for Gemma. Without the Perso-Arabic `hi_in` clip, Hindi is 55.7 (reference) against 43.7 (ASR) for Hy-MT2 and 59.4 against 44.6 for Gemma.

### Critical-error candidate flags

| Run | Outputs | Number | Negation | Entity | Outputs with any flag |
| --- | ---: | ---: | ---: | ---: | ---: |
| Hy-MT2, reference text | 28 | 0 | 1 | 12 | 13 |
| Hy-MT2, ASR text | 27 | 1 | 0 | 17 | 17 |
| Gemma 4 E2B, reference text | 28 | 0 | 0 | 10 | 10 |
| Gemma 4 E2B, ASR text | 27 | 1 | 0 | 17 | 17 |

Examples (source, output, reference):

- **Negation flag, likely correct.** Hy-MT2, `sw_ke` reference text 264. Source: `Kamishna alisema, “Bado hatujakubaliana kuhusu kanuni za chanzo ...”`. Output: `Kamishna said, “We have discussed the rules regarding the order and arrangement of the items, but the system we have is ready to start trading on July 1, 2020.”` Reference: `The commissioner said, "We haven't yet agreed on rules of origin ...`. "Not yet agreed" became "have discussed". Gemma kept the negation on the same input.
- **Number flag, likely correct, from propagated ASR error.** Hy-MT2, `sw_ke` ASR 264. The recognizer wrote the date as garbled words (`mmano julae Sare moja Na kwa wa elfu mbilina Shering`). Output: `Mishina said that now it's time to go. I've been waiting for this moment for a long time. But I don't want to engage in business anymore. ...` The date is gone and the text is invented. Gemma on the same input produced `... one by one, for a thousand million shillings.`, also flagged.
- **Severe error with only an entity flag.** Hy-MT2, `ar_eg` reference text 1087. Source: `يقع النمر في نفس المجموعة (جنس النمور) مع الأسود، والنمور المرقطة، واليغور. هؤلاء الأربعة هم الوحيدون اللذين يمكنهم الزئير.` Output: `The tiger belongs to the same group as the black panther, the rosette panther, and the yiguur. These four are the only ones that can fart.` Reference: `... as lions, leopards, and jaguars. These four cats are the only ones who can roar.` The ASR-input version ended `can excrete urine`. Only the dropped "Panthera" was flagged; the meaning error on "roar" passes every heuristic. The earlier Arabic check cited in the decision record used this same recognized sentence. Gemma translated the same source as `These four are the only ones that can roar.`
- **Unflagged meaning change.** Hy-MT2, `hi_in` reference text 1087: `There are only four species of lions that can roar.` for a source meaning "only these four cats can roar". No flag fired.
- **Script substitution passed through.** Both models, `hi_in` ASR 773, where the recognizer wrote Perso-Arabic script (`ورلڈ سفیر ...`): Hy-MT2 `The World Ambassador is generally a major representative of World X Position ...`, Gemma `The World Ambassador is generally a big enthusiast of the World X position ...`. Both produced fluent English with no relation to the reference `A World's Fair (commonly called World Exposition, or simply Expo) ...`.
- **Propagated ASR error.** Hy-MT2, `hi_in` ASR 1087: the recognizer heard `भागो` ("run") for `बाघों` ("tigers"), and the output began `Run away, those jackals and hyenas!`
- **Likely false positive.** Gemma, `pt_br` reference text 831: `you must have an identity document with validity` for the reference `valid ID`, flagged as a dropped entity. Most entity flags are of this kind (`ID` against "identification", `Fair` or `Exposition` against "Expo" or "exhibition", or an omitted parenthetical `Panthera`).

### Timing

Wall time per sentence covers process start, model load, prompt evaluation and generation. The whole batch of 112 processes ran from 13:34:37 to 14:01:17 (-07:00), 26 min 40 s, with no deadline reached and every process exiting 0 with non-empty output.

| Run | Sum (s) | Median (s) | 90th percentile (s) | Max (s) | Median generation ms per token | Median generated tokens |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Hy-MT2, reference text | 237.2 | 5.8 | 14.9 | 39.6 | 63.7 | 34 |
| Hy-MT2, ASR text | 544.0 | 10.9 | 59.1 | 117.7 | 168.5 | 33 |
| Gemma 4 E2B, reference text | 391.5 | 13.9 | 21.2 | 28.0 | 160.9 | 30.5 |
| Gemma 4 E2B, ASR text | 424.9 | 12.7 | 24.7 | 43.6 | 166.6 | 29 |

The host was shared throughout. Total CPU was sampled at 90 to 100%, with a VMware virtual machine, a separate `whisper-cli` recognition process, Rust builds and other Python processes running; this study's scorer build overlapped part of the Hy-MT2 reference pass. The same Hy-MT2 probe sentence took 2.9 s per generated token under heavier load before the batch and 64 to 170 ms per token during it. Timing is indicative only and supports no latency or capacity claim.

## Findings

- Both models produced English for every input. On reference text, Gemma 4 E2B scored higher than Hy-MT2 on five of seven languages and in total (59.7 against 54.7 chrF++), with the largest gaps on Arabic and Swahili; Hy-MT2 was higher on Spanish and Mandarin. With four sentences per language, these differences are within the likely sampling noise and do not rank the models.
- Swahili, undeclared for Hy-MT2, was its weakest language (38.6 chrF++ on reference text, 27.0 on ASR text) and produced a negation flip and an invented narrative. This supports the worker's rule that an undeclared language stays untranslated for a profile that does not declare it.
- Recognition errors propagate. ASR input cost 8.3 (Hy-MT2) and 9.4 (Gemma) chrF++ points on a matched set, and far more for Hindi and Swahili, where turbo had the highest error. A script substitution by the recognizer produced fluent, unrelated English from both models. Translation must not hide the recognizer's language evidence, and a detected `ur` on expected Hindi should not be translated as if it were correct text.
- The most severe errors, such as "roar" becoming "fart" and a changed quantifier ("only four species of lions"), passed all three heuristics. The heuristics caught one clear negation flip and the dropped dates, but most flags were acceptable paraphrases of names. They are a triage aid, not an error detector, and a calibrated model judge or published human error annotations are still needed for critical-error evidence.
- Gemma 4 E2B cannot use the worker's current conversation-mode invocation on this build: `--reasoning off` still injects the thinking directive. A Gemma profile would need its own template that renders the prompt without a system thinking turn, as done here.
- Hy-MT2 added content once (`(Panthera leo)` on Mandarin 1087), and both models sometimes changed names and quantities that single-reference metrics penalize only slightly.

## Limitations

This is read speech from a benchmark corpus, not broadcast audio. There are four sentences per language (three for Spanish ASR), and only four distinct English references across all languages, so per-language scores have very wide uncertainty and sentence-level BLEU is unstable. FLEURS non-English sentences are themselves translations and can diverge from the English (the Hindi 1087 source makes the lion the subject), which single-reference metrics count as error. The sentences come from the train split, and both models may have seen FLEURS or its FLoRes source text in training. There was no human review; the observations above come from reading outputs against references during this run and are not a reviewed judgment. Flags are heuristic, English-only and tuned to nothing. Each sentence was translated alone, with no neighboring context. Canadian French, Navajo and Klingon are not covered. The harness did not bound memory or clear the environment, and it ran on a heavily shared host, so timing is indicative only.

## Workspace

Scratch files are under `.agents/language-evaluation/mt/eval/`: `items.json` (SHA-256 `6c9ed6f3299c0089f80d798df785eeb29c4a1ec485bf1c7305622e01dd11447d`), `translations.jsonl` with raw outputs and per-process timings (`d655a569ac0ab86aecf1cb328ad4182c2c65d4e24cbe1853be3e0935e8990fe5`), `scores.json` (`bdc78dc87c0d25b5c70149d6f96f7a5be726dd9354135b92363b14e7868b1217`), per-process logs, the runner and the aggregation script.
