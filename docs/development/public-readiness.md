# Public-source readiness

Updated: 2026-09-22. Status: preparing the first public GitHub source checkpoint. No application release or crates.io publication is implied.

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

Final scanner results, GitHub feature readbacks, cache handling, anonymous access, and the hosted verification result must be recorded before this document calls publication complete.

## Registry boundary

The original workspace `cargo publish --dry-run -p sigy` failed because the internal path dependency has no registry version. Publishing those internal packages also requires resolving the root-only SQLite patch: a downstream package does not inherit that workspace patch. Adding version strings alone would not preserve the qualified native engine.

The current workspace packages explicitly disable registry publication until packaging is qualified. Only `sigy` needs a reserved public name for now; no requirement to reserve or publish internal package names has been established. A separate documentation-only `sigy` 0.0.0 reservation artifact passed its dry-run locally, but the user deferred uploading it. It is not an installable application and has not been published by this work.

## Product qualification remains separate

The [progress record](progress.md) owns actual test evidence. Native memory/CPU isolation, physical power-loss behavior, migration backups, platform support, language quality, sustained capacity, modern cryptographic profiles, and the complete first-release journeys remain explicit gates. Keep SQLite for the transactional catalog under the [storage reassessment](../../research/08-storage-and-evidence.md); source publication does not require a database migration.
