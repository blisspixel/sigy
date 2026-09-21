# Autonomous monitoring and music intelligence

Reviewed: 2026-09-20. Status: research-informed design. Autonomous monitoring belongs in the first release; music identification follows it.

## Foundation for autonomous monitoring

Radio Browser offers useful source-selection metadata, but station tags and click counts do not measure current subject coverage. A monitor must combine directory hints with actual recorded observations. [Directory API](https://docs.radio-browser.info/).

Structured model outputs can constrain the shape of a proposed monitoring plan. They do not validate source availability, resource affordability, or truth of generated findings. [Structured output capability](https://docs.ollama.com/capabilities/structured-outputs).

Full-text retrieval is available in candidate catalog technology and provides a testable baseline before adding embedding/reranking complexity. Multilingual retrieval still needs its own corpus and evaluation. [FTS5](https://sqlite.org/fts5.html).

## Proposed monitoring method

Translate the goal into a typed plan, validate it against policy, and collect within reserved limits. The model can propose source changes; deterministic execution controls their admissibility. The same separation applies to paid calls.

Maintain a source-selection history and an exploration allowance so the monitor can discover useful sources without repeatedly narrowing to the same station. Record source diversity, observed topical relevance, availability, and excluded candidates. Cooldowns and explicit change thresholds prevent frequent source switching.

Evaluate discovery and analysis independently. A useful summary of one station does not prove the discovery system covered a topic well. A large list of stations does not prove the system transcribed them successfully.

For each report, retain the evidence set, collection window, processing coverage, deduplication method, and uncertainty. Track repeated stories and potentially shared/syndicated feeds separately from independent observations. Topic change over time must account for changes in the monitored sample.

No source text can grant the monitor new permissions. Station descriptions, transcripts, retrieved passages, and model-generated plans remain data until validated by the application's policy rules.

## Music evidence

Chromaprint describes its focus as identification of near-identical audio, with tradeoffs and target uses including file identification, duplicate detection, and long-stream monitoring. That is insufficient evidence that it will reliably identify arbitrary short, noisy broadcast clips against a broad catalog. [Upstream description](https://github.com/acoustid/chromaprint).

The public AcoustID service documents fingerprint lookup, application registration, a three-request-per-second limit, and a non-commercial-use restriction for its free service. A commercial deployment needs the appropriate service arrangement. [Service documentation](https://acoustid.org/webservice).

MusicBrainz provides recording/artist metadata and identifiers, with documented rate limits and service-use conditions. Its API and underlying data licensing are separate considerations. [API](https://musicbrainz.org/doc/MusicBrainz_API), [data licenses](https://musicbrainz.org/doc/About/Data_License).

AudD documents short-clip recognition and a separate audio-stream monitoring API. It is a commercial-service candidate to compare with local lookup. Its documented endpoints and catalog claims do not demonstrate regional match quality in Sigy's intended corpus. [AudD documentation](https://docs.audd.io/).

ACRCloud documents identification and separate music, custom-file, live-channel, and music/speech result schemas. It is another candidate for a measured provider comparison. Verify the actual endpoint contract, permitted use, regional coverage, costs, and data destination before qualification. [Identification API index](https://docs.acrcloud.com/reference/identification-api).

## Music pipeline proposal

1. Capture raw station track metadata with its observation time and original spelling.
2. Identify candidate music intervals and retain their link to audio.
3. Query a configured fingerprint/catalog provider or a local reference collection.
4. Reconcile candidate identities, versions, overlapping detections, and metadata conflicts.
5. Form play intervals with explicit start/end certainty and detection method.
6. Aggregate into sampled-airplay views with coverage and identification rates.

Do not have a language model guess track identity from a vague transcript or assume every metadata update is a new play. Cached lookup results and deduplicated queries control cost, but cached results must preserve catalog/version context.

| Identification approach | Advantage | Limitation |
| --- | --- | --- |
| Station metadata | Low processing cost, often immediate | Missing, delayed, malformed, or promotional text |
| Local fingerprints/reference catalog | Offline operation and controlled matching | Catalog coverage, storage, index maintenance |
| Hosted identification | Potentially broader catalog | Fees, clip transfer, service limits, coverage differences |
| Manual annotation | Useful for unknown tracks and evaluation | User effort; should remain a correction with provenance |

Provider selection remains open. Paid music lookup participates in the same budget mechanism as model calls.

Begin paid integration evaluation with bounded clip requests. A provider-hosted continuous monitor may keep billing when Sigy is offline; it needs verified expiry or a finite provider-enforced liability bound before autonomous use. Do not infer a strict cap from a local request counter or a successfully sent stop request. Prices and contractual limits remain open and must be obtained for the chosen plan before a paid experiment.

Most intended music is non-English. Matching need not depend on lyric transcription, but catalog coverage must be established for the intended regions and languages. Preserve original-script names, aliases, transliteration provenance, and uncertain language metadata separately. Station country, artist origin, and sung language are independent dimensions. See [Multilingual processing](10-multilingual-processing.md).

## Ranking contract

Specify geography, languages, date/window, eligible stations, actual captured time, and identification coverage. Provide play count, distinct stations, airtime, and rates per monitored hour separately. Keep per-country views available so a large station sample in one country does not silently define an entire continent.

The sample is usually selected adaptively, not randomly. Do not attach population-level statistical confidence or call it a national/continental chart without a defensible sampling method or an external chart source. Present the output as observations from the monitored sample.

## Required evaluation

Build labeled topic windows with supporting and contradictory passages, duplicated news, irrelevant mentions, and adversarial instructions inside source content. Measure retrieval recall, claim support, missed topics, duplicate findings, coverage reporting, and policy/cost adherence.

For music, use predominantly non-English examples and evaluate exact matches, local/independent releases, multilingual songs, versions/remixes, speech over music, crossfades, short clips, station compression, unidentified music, duplicate feeds, and repeat plays. Report false matches and unknown rates separately by region, language where known, and genre. Retain unidentified airtime in coverage denominators. Broad catalog claims need evidence from the intended countries and genres.

## Near future

The [analysis design](../docs/planning/10-analysis-and-knowledge.md) specifies weekly window boundaries, deterministic counts/rates, stable station panels, and revisions. Optional [decision models and local classifiers](16-decision-models-and-classifiers.md) can organize transcripts or known music metadata; they do not identify songs or calculate popularity. [Agentic analysis](15-agentic-analysis.md) extends the bounded monitoring and evidence evaluation plan.

Treat new music catalogs, speech models, and agent frameworks as replaceable candidates. The stable product assets are retained observations, typed plans, coverage history, and validated evidence relationships.
