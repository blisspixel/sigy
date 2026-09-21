# Universal interpretation: languages, symbols and mathematics

Reviewed: 2026-09-20. Status: primary-source research and implementation implications, no inference or accuracy measurement performed. [Product contract](../docs/design/signal-interpretation.md).

## Model coverage is a route map, not the product boundary

Omnilingual ASR documents broad recognition and adaptation with paired examples, including v2 CTC/LLM families. It is a broad-coverage candidate beyond a single familiar recognizer. Its reference implementation does not settle native deployment; export/model revision parity, memory, and license evidence still matter. [Official project](https://github.com/facebookresearch/omnilingual-asr), [native-runtime research](17-local-processing-and-capacity.md).

Inspection of the supported-language file and result table at upstream commit `81f51e224ce9e74b02cc2a3eaf21b2d91d743455` found French `fra_Latn`, but no `nav_Latn` or `tlh_Latn`. Do not infer Navajo or Klingon coverage from the headline count. Address the gap with additional candidates or adaptation. [Language identifiers](https://github.com/facebookresearch/omnilingual-asr/blob/81f51e224ce9e74b02cc2a3eaf21b2d91d743455/src/omnilingual_asr/models/wav2vec2_llama/lang_ids.py), [per-language results](https://github.com/facebookresearch/omnilingual-asr/blob/81f51e224ce9e74b02cc2a3eaf21b2d91d743455/per_language_results_table_7B_llm_asr.csv).

A community M2M100 Klingon model is a concrete local text-translation lead. Metadata at revision `06c427e8921ddfedf173fc47afd54b205690ef10` declares English/Klingon/Romanian, MIT licensing, a custom 33k dataset, and training loss. That loss is not held-out translation evidence. Review training-data rights, vocabulary changes, translation directions, independent evaluation, and native export before adoption. No weights were downloaded. [Publisher model](https://huggingface.co/MihaiPopa-1/M2M100-418M-Klingon), [metadata](https://huggingface.co/api/models/MihaiPopa-1/M2M100-418M-Klingon).

Navajo research should include community and academic resources, with permission for any recordings, paired data, or evaluation use. Public teaching material does not imply model-training or redistribution rights. [Navajo Language Academy](https://navajolanguageacademy.org/nla.htm), [University of New Mexico resources](https://ling.unm.edu/navajo-language-program/resources.html). No suitable production-grade Navajo ASR route has yet been established by this pass. Keep it as a required engineering workstream.

## Beyond speech translation

Qwen3.6-27B's official card describes document, visual and reasoning capabilities. It is a candidate for visual symbols and mathematics, not proof of arbitrary-language translation or correctness. Its size requires a bounded host-capacity experiment. llama.cpp's `mtmd` provides native multimodal inference for specific models; verify family and quantization compatibility individually. [Model card](https://huggingface.co/Qwen/Qwen3.6-27B), [native tooling](https://github.com/ggml-org/llama.cpp/tree/master/tools/mtmd). These examples do not select a model or runtime.

MathML distinguishes presentation and content structures. The distinction matters even if Sigy's first interchange format is a smaller typed expression tree: notation and asserted semantics are separate records. Do not evaluate arbitrary embedded code or invoke a TeX engine just to understand an expression. [MathML 3 Recommendation](https://www.w3.org/TR/MathML3/).

Z3 reasons over formal constraints. Such a solver can verify a proposed formalization inside bounded resources. It cannot supply missing source meaning; a satisfiable model does not establish a real-world claim. Solver choice, isolation, packaging, input limits and theory coverage remain integration decisions. [Official guide](https://microsoft.github.io/z3guide/).

Known packet decoders, checksums, Morse timing, units, supplied-key authentication, and schemas offer other checkable stages. Meaning inferred from an unknown alphabet or numeric pattern remains a hypothesis until further evidence constrains it. Preserve alternatives instead of forcing all observations into one transcript string.

## Bounded evaluation work

1. Build a licensed corpus containing the three minimum named languages, ordinary high-volume languages, code switching, acoustic faults, and unknowns. Evaluate ASR, translation and interpretation separately.
2. Test native broad-ASR and specialist routes. For uncovered cases, evaluate paired examples, terminology and adaptation methods; estimate data, compute, memory and review needs before running them.
3. Test text/image mathematics with independently verified expressions, ambiguous glyphs, contradictory units, and missing definitions. Separate recognition, formalization and solver errors.
4. Route known encodings, telemetry, timed symbols, and unknown sequences through the common artifact/transform pipeline. Require exact known-fixture recovery and useful alternatives/abstention for ambiguity.
5. Embed malicious instructions in each representation. Decoded messages, equations, documents and transcripts remain data; they cannot authorize tools, destinations, downloads, keys or spending.
6. Measure whether users reach useful meaning, inspect evidence, supply context, and understand unresolved points. A large language list or fluent explanation alone does not meet this gate.
