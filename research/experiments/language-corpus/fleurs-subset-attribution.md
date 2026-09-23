# FLEURS pilot attribution and provenance

Reviewed: 2026-09-23. This packet records source attribution and retained license evidence for the [three-clip calibration pilot](three-clip-pilot.json). The selected audio has not been acquired, decoded, or recognized. Keeping this packet is a project provenance requirement; it is not a claim that private downloading requires a separate attribution packet or retained license file.

## Dataset and supplied credits

Dataset: **FLEURS: Few-shot Learning Evaluation of Universal Representations of Speech**, published in the [google/fleurs dataset repository at revision `70bb2e84b976b7e960aa89f1c648e09c59f894dd`](https://huggingface.co/datasets/google/fleurs/tree/70bb2e84b976b7e960aa89f1c648e09c59f894dd).

The pinned card requests this citation: Alexis Conneau, Min Ma, Simran Khanuja, Yu Zhang, Vera Axelrod, Siddharth Dalmia, Jason Riesa, Clara Rivera, and Ankur Bapna (2022). [FLEURS: Few-shot Learning Evaluation of Universal Representations of Speech](https://arxiv.org/abs/2205.12446). arXiv:2205.12446.

The card acknowledges [patrickvonplaten](https://github.com/patrickvonplaten) and [aconneau](https://github.com/aconneau) for adding the dataset. It identifies the [FLoRes machine translation benchmark](https://arxiv.org/abs/2106.03193) as the source of the parallel sentences. These are upstream credits and lineage, not project authorship or endorsement. The card does not supply a named copyright holder; none is inferred from its creator-category metadata.

The card's YAML declares **Creative Commons Attribution 4.0 International (CC BY 4.0)**. Retain that declaration, the supplied credits, and the [license link](https://creativecommons.org/licenses/by/4.0/) with any shared subset. FLEURS material retains its own license; the project's Apache 2.0 license does not replace it.

## Retained source evidence

Both raw documents are retained unchanged in ignored local experiment storage. Their hashes below identify the exact copies reviewed, independent of how a Markdown renderer displays them. The public repository contains this record, not the dataset audio or the retained source-document bodies.

| Document | Official source | Retained bytes | SHA-256 |
| --- | --- | --- | --- |
| FLEURS dataset card | [Revision-pinned README](https://huggingface.co/datasets/google/fleurs/blob/70bb2e84b976b7e960aa89f1c648e09c59f894dd/README.md) | 385,614 | `688f79f2a5c731af3796e9f683eb02f9b3f09d040decd8c5625d0f37098e71c6` |
| CC BY 4.0 legal text | [Official legalcode.txt](https://creativecommons.org/licenses/by/4.0/legalcode.txt) | 18,657 | `9ba9550ad48438d0836ddab3da480b3b69ffa0aac7b7878b5a0039e7ab429411` |

The card's locally computed Git blob ID is `dcc0872174e54ad416dee938651d777378d4ba4f`, matching its observed HEAD and GET ETags. See the [card acquisition record](card-provenance-pilot.md). The original card remains unchanged, including the incomplete closing syntax in its supplied BibTeX; the prose citation above preserves the supplied bibliographic details.

The legal text came from one bounded, no-redirect HTTPS GET of the official fixed URL. Its received length and SHA-256 match both payload and publication receipts. The retained UTF-8 document contains the introductory notices, Sections 1 through 8, and the closing Creative Commons notice and contact line. Its source URL has no immutable revision, its weak HEAD ETag is not a content hash, and no independent expected content hash was supplied. Structural inspection and clean EOF do not establish an independently authenticated complete upstream version. This record freezes the reviewed copy without making that stronger claim.

The legal-text attempt retains its full 131,073-byte reservation despite receiving 18,657 bytes. Together with all three card reservations and the explicitly unmeasured historical policy charge, the shared ledger charges 10,738,066,000 bytes. Those are conservative accounting charges, not measured wire traffic.

## Selected material and changes

The pilot manifest SHA-256 is `79dd334fb1665a1bf3493aa88d69650ae8ea6f69b31fe843f981f11296753b80`. It selects three recordings from the frozen [112-clip screening manifest](fleurs-screening-manifest.json), SHA-256 `487632ee83967f868afa6313d54f544a32cb07075d3b446701b05af870389fd9`.

| Corpus label | Exact selected recording | Sentence group | Partition | Declared duration |
| --- | --- | --- | --- | --- |
| Arabic, `ar_eg` | `ar_eg/train/16932136382444340406.wav` | 1087 | Calibration | 10.32 seconds |
| Hindi, `hi_in` | `hi_in/train/2934636319415126502.wav` | 264 | Calibration | 12.96 seconds |
| Spanish, `es_419` | `es_419/train/9301998606286834650.wav` | 773 | Calibration | 11.34 seconds |

The declared total is 34.62 seconds. Source locale labels describe the corpus sampling frame, not verified accents, dialects, speaker identities, or recognizer capabilities. The pilot and [archive acquisition manifest](archive-acquisition.json) retain each original archive path, declared size, and publisher LFS SHA-256. Individual WAV hashes and actual decoded properties remain unknown until verified acquisition and extraction.

The project change so far is selection of these three metadata identities from the upstream corpus. No real audio transformation has occurred. Future extracted originals must retain exact member names and hashes. Each decoded, resampled, trimmed, normalized, or otherwise modified derivative must record its input identity and changes while preserving the retained original and upstream notices. Calibration labels remain attached to derived fixtures; this packet does not reclassify them as holdout material.

## License conditions and remaining review

Section 3 of the [official legal text](https://creativecommons.org/licenses/by/4.0/legalcode.txt) sets attribution conditions when licensed material is shared. Preserve supplied creator and designated-party identification, copyright notices, license notices, warranty-disclaimer notices, and source links where reasonably practicable. Indicate modifications and retain indications of previous modifications; include the license text or its link. The license permits reasonable attribution methods appropriate to the medium and context. This packet supports those records without claiming to replace the license.

Section 2 prohibits implying endorsement and limits the rights granted, including separate treatment of privacy, publicity, patent, and trademark rights. Section 5 supplies warranty and liability limitations. Preserve the full retained legal text and any additional notices found during archive inspection. Missing notices in the card are not evidence that an uninspected archive has none.

This packet documents the reviewed card, legal text, and selected identities. Archive/member verification, additional-notice inspection, safe extraction, evaluator-only reference handling, decoding, and recognition remain separate work. Provider uploads and provider-specific terms require separate review. Nothing in a source document or this packet opens an asset-download, model-execution, or paid-processing gate.
