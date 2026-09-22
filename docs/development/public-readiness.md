# Public-source readiness

Updated: 2026-09-22. Status: [GitHub source is public](https://github.com/blisspixel/sigy), with local and hosted verification passed. No application release or crates.io publication is implied.

## Scope and authorization

The user authorized making `blisspixel/sigy` public after credential and repository due diligence, then requested GitHub first and crates.io later. Registry publication is deferred. The active language-pipeline goal continues independently of source visibility.

Review covers intended tracked content, reachable history and local refs, commit identities/messages, repository metadata, Actions logs and artifacts, screenshots, license provenance, installation instructions, and package boundaries. Credential-pattern review is evidence within that scope, not proof that every possible secret format has been recognized.

## Review and cleanup

- The initial history review covered 39 commits and 862 unique blobs, with one branch and no tags, pull-request refs, submodules, or symlinks. No operational credentials, environment files, user libraries, captured media, model weights, or private source endpoints were identified. The retained TLS private-key fixture is deliberately public loopback test material, not an operational credential.
- The final source review included 301 intended files, including new source and documentation, plus all 39 existing commits and 862 reachable blobs. It found no operational credentials or prohibited public-attribution markers. Examples containing source credentials or query tokens use synthetic rejection fixtures and documentation placeholders; they are not configured endpoints.
- Gitleaks 8.30.1 independently scanned all 39 reachable commits across the three local refs and a stable 301-file intended tree. Each scan reported the same 16 generic-key candidates: 14 SQLite/SQLCipher C expressions or API macros and two planning-table prose rows. All were reviewed as false positives, with historical lines compared to current source. No unresolved credential candidate or scanner warning remained. Default rules and built-in filters were unchanged; no project allowlist, baseline, fingerprint ignore, or disabled rule was added, and inline allow comments were disabled. Reports contain locations and rule IDs only. Pattern scans do not establish coverage of binary key formats; the documented public DER fixture was reviewed separately.
- The Actions review covered 31 runs and 32 attempts, including failed and cancelled logs. No repository secrets/variables, published artifacts, releases/assets, deployments, environments, issues, pull requests, or comments were present at that review. Pages and an actual wiki were absent. Six derived build caches were inventoried and deleted before publication; their archive contents were not independently scanned. Future public builds recreate their own caches.
- Three personal temporary-directory prefixes in a terminal experiment receipt were redacted from current source while preserving the failure evidence. Earlier commits retain those benign paths. No credential rotation or history rewrite was indicated by the findings. History has not been rewritten.
- Apache 2.0 and vendored license/provenance notices remain intact. Public authorship remains the repository's declared owner. Personal libraries, credentials, local environment files, and build/experiment directories have explicit ignore rules; ignoring a path does not remove already tracked content.
- [SECURITY.md](../../SECURITY.md) and [CONTRIBUTING.md](../../CONTRIBUTING.md) provide reporting and contribution routes. CI uses pinned actions, read-only contents permission, and checkout without persisted Git credentials. Pull requests run the same verification command; native-media qualification stays local.
- The README labels the early development state, historical screenshot, actual Windows evidence, and missing speech recognition and translation. A source checkout remains the installation path. No supported platform matrix or certification is claimed.

## Publication checks

GitHub visibility was changed to public only after local verification and the reviews above. The verified source checkpoint is `8aa072d72cd39a0693199e68ca1ab1315215fae6`. API readback confirmed public visibility, secret scanning enabled, push protection enabled, and private vulnerability reporting enabled. The initial secret-alert query returned no alerts; that response is not proof that every background scan has completed or every secret format is recognized.

Anonymous API access succeeded. A clone with credential helpers and extra HTTP authorization headers disabled resolved to that exact checkpoint. Anonymous downloads of both installer scripts matched their committed Git blob hashes. This verifies source and installer retrieval, not a clean-machine installation or additional operating-system support. Six pre-publication build caches were removed and the cache list was empty before pushing the public checkpoint.

Local `cargo verify` passed 273 ordinary tests and `cargo verify-media` passed all 12 native-media tests. The [first public Windows workflow](https://github.com/blisspixel/sigy/actions/runs/35796307780) passed `cargo verify` at the source checkpoint above on a fresh standard hosted runner. Native-media tests remain local. The workflow used no paid runner or new spending allocation. Local models and paid providers were not run.

CI maintenance reviewed on 2026-09-22 replaces the Node 20 actions with commit pins for [checkout v7.0.1](https://github.com/actions/checkout/releases/tag/v7.0.1) and [rust-cache v2.9.2](https://github.com/Swatinem/rust-cache/releases/tag/v2.9.2). Both declare Node 24. GitHub schedules [Node 20 removal for 2026-09-23](https://github.blog/changelog/2025-09-19-deprecation-of-node-20-on-github-actions-runners/). Release tags were resolved to commits, including the annotated Rust cache tag, and GitHub reported valid commit signatures. The toolchain action is composite and needs no Node runtime change. Token permissions and credential persistence settings are unchanged. The first public run above used the previous pins; hosted verification of these replacements remains pending.

## Registry boundary

The original workspace `cargo publish --dry-run -p sigy` failed because the internal path dependency has no registry version. Publishing those internal packages also requires resolving the root-only SQLite patch: a downstream package does not inherit that workspace patch. Adding version strings alone would not preserve the qualified native engine.

The current workspace packages explicitly disable registry publication until packaging is qualified. Only `sigy` needs a reserved public name for now; no requirement to reserve or publish internal package names has been established. A separate documentation-only `sigy` 0.0.0 reservation artifact passed its dry-run locally, but the user deferred uploading it. It is not an installable application and has not been published by this work.

## Product qualification remains separate

The [progress record](progress.md) owns actual test evidence. Native memory/CPU isolation, physical power-loss behavior, migration backups, platform support, language quality, sustained capacity, modern cryptographic profiles, and the complete first-release journeys remain explicit gates. Keep SQLite for the transactional catalog under the [storage reassessment](../../research/08-storage-and-evidence.md); source publication does not require a database migration.
