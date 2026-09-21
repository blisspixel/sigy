# Product naming research

Reviewed: 2026-09-20. Decision updated: 2026-09-21. Status: historical preliminary screening. The user chose to keep Sigy for now; further naming exploration is deferred. No rename, domain purchase, package reservation, or trademark clearance has occurred.

## Direction

The public repository list includes `primr`, `deepr`, `distillr`, `sealr`, `fitr`, `viewr`, `privr`, and `retonr`, alongside longer names such as `numinous`. The recurring pattern is a short, readable concept with compact spelling, often ending in `r`. This is an observation, not a requirement to force every new name into that spelling. [Public repository inventory](https://api.github.com/users/blisspixel/repos?per_page=100&sort=updated), [profile](https://github.com/blisspixel).

The current priority is immediate intelligibility: the name should give someone a useful first guess about the product. The user found abstract names such as Exnil uninformative, proposed SignalSift, and then suggested Signeta or a similar name. A compact coined name remains possible when its connection to signals is recognizable. Something meaningful emerging from apparent emptiness remains an optional visual/theme direction, rather than the primary naming criterion.

Prefer a short ASCII command, a pronounceable spoken name, and enough distinctiveness to search for documentation. Avoid obvious confusion with established apps or companies, especially in media, AI, communications, and security. Assess exact spelling separately from pronunciation and near-name collisions.

## Current shortlist

| Candidate | Intended association | Assessment from this pass |
| --- | --- | --- |
| Signeta | Compact signal-associated name; suggested pronunciation "sig-NET-uh" | Current user suggestion. More informative than the abstract candidates, although it can also suggest signatures. Signetta is an existing document-workflow platform with nearly identical spelling and likely spoken confusion. A historical exact-name software-company listing also exists; current operating status is unverified. |
| Signaleta | Retains the complete word "signal" within a coined product name | Suggested nearby alternative. No obvious exact-name app/company surfaced in quoted web search. Longer than Signeta, but the signal association is clearer. Repository, package, domain, and trademark checks have not been performed for this candidate. |
| SignalSift | Collect signals and sift them for useful information | Proposed by the user; strong descriptive fit across source types. Multiple exact-name intelligence/analysis products exist, so it conflicts with the desired separation from existing apps. Not adopted. |
| RadioSift | Radio listening plus finding useful information | No obvious exact-name app/company surfaced in this bounded pass. GitHub repository search returned zero results. Clearer category cue, but narrower than future feeds and general signal analysis. |
| SignalGlean | Extract useful information from signals | No obvious exact-name app/company surfaced in quoted web search; GitHub repository search returned zero results. Longer, and "glean" may be less immediately understandable than "sift." Not proposed by the user or selected. |
| Sigy | Short, friendly signal association | Strong product fit, but already used for security research and a software library. It is not a clean software namespace. |

Earlier abstract candidates Exnil, Nilr, Echyr, and Sigyr are superseded as the recommended direction because their purpose was not clear enough. A descriptive subtitle may explain features, but it should not have to supply the entire meaning of an opaque name. No tagline has been adopted.

## Significant collisions found

| Name | Existing use | Consequence for this project |
| --- | --- | --- |
| Signeta / Signetta | [Signetta document-workflow platform](https://www.signetta.eu/en); [Signeta company listing](https://www.cbinsights.com/company/signeta) | Signetta is a close software-brand spelling and pronunciation collision. The exact-name Signeta listing is secondary evidence; its current operating status was not independently verified, and signeta.com could not be retrieved. Neither finding alone establishes popularity or trademark rights. |
| SignalSift | [Feedback analysis](https://www.signalsift.io/), [privacy-violation intelligence](https://www.signalsift.org/), and [terms for a Reddit demand-analysis service](https://signalsift.dev/terms) | Exact-name overlap in analysis and intelligence. These pages establish public use, not independently verified product performance, popularity, or trademark rights. |
| WaveSift | [RF modulation-classification CLI](https://github.com/anikaitj/wavesift) | Direct technical and product overlap with the signal-analysis roadmap |
| Sigy | [Signal-related security research](https://ahoi-attacks.github.io/sigy/) and [function-signature library](https://github.com/timothycrosley/sigy) | Exact software overlap, with particularly relevant search ambiguity around signals/security |
| Signy | [Electronic document service](https://signy.online/) | Existing application brand; does not improve distinctiveness |
| Signaly | [Trading-signals application](https://signaly.me/homepage) and [sales platform](https://signaly.nl/) | Existing software brands and an overlapping signals/analysis association |
| Voidr | [AI enterprise software support](https://www.voidr.co/en) | Strong conceptual fit but a direct software-company collision |
| Aethr | [AI/data business](https://aethr.ai/about/) and [creator-management service](https://useaethr.com/privacy) | Crowded technology name |
| Glymr | [Knowledge and AI-readiness consultancy](https://glymr.com/about/) | Relevant knowledge/analysis brand collision |
| Siglr | [LoRa-related product identifier in IoT documentation](https://signetik.github.io/sigcell-api-docs/) and an existing npm package | Particularly poor separation from the hardware roadmap |
| Sigtrail | [Commit-signing audit CLI](https://github.com/maheshrijal/sigtrail) | Exact existing command-line project |
| Tunr | [Music, radio, podcast, and visualizer app](https://apps.apple.com/us/app/tunr-music-player-visualizer/id948831179) | Direct product overlap; avoid similarly pronounced Tunyr as well |

Signlr and similar compressed spellings also risk confusion with [SignalR](https://learn.microsoft.com/en-us/aspnet/core/signalr/introduction), independent of exact spelling availability. These are practical naming judgments, not legal conclusions.

## Screening method and limits

Read the public repository inventory, searched quoted names alone and with app/software/company qualifiers, inspected primary product pages, and queried public GitHub repository search. Search queries using `in:name` also return substring or owner matches; result totals are not counts of exact-name competing products. Results were limited to a first page of up to ten repositories per candidate, so absence from those results is not exhaustive clearance.

Exact [crates.io registry](https://crates.io/) and [npm registry](https://www.npmjs.com/) API requests for `radiosift` and `wavesift` returned HTTP 404 on the review date. This only means those exact endpoints did not return a package record at that time. WaveSift's existing RF project demonstrates why an absent package record does not establish a clean name. These checks do not reserve a namespace, establish publishability, select Rust/JavaScript, or prove trademark/domain availability. Endpoint forms checked were `https://crates.io/api/v1/crates/{name}` and `https://registry.npmjs.org/{name}`. SignalGlean's package namespaces were not checked.

Before public release, check intended distribution namespaces, desired domains through registration data, confusingly similar product names, relevant trademark records, and pronunciation/meaning with speakers of priority languages. No domain or trademark-register check has been completed. Keep this bounded screening distinct from legal clearance. The decision to keep Sigy is recorded in [D-01](../docs/planning/05-delivery-and-decisions.md).
