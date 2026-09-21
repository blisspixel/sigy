# Security, privacy, and release design

Last updated: 2026-09-21. Status: proposed controls derived from the product's source, service, model, and extension boundaries. This is not a completed security assessment.

## 1. Properties to preserve

Protect library integrity, operational credentials and keys, local host access, user-defined processing destinations, authorized spend, and control of running jobs. A malformed station, model response, packet, or imported artifact must not become control authority.

The proposed default is a single-user installation with local control. Remote providers are configured capabilities; they do not imply remote access to Sigy's service. A future multi-user or remotely exposed installation requires a separate access-control design.

## 2. Boundary controls

| Boundary | Proposed controls | Verification |
| --- | --- | --- |
| Directory/source to network fetch | Allowed schemes/destinations, redirects and nested resources checked, bounded size/depth/time | Redirect chains, rebinding, private-address and malformed-playlist fixtures |
| Media/signal input to decoder | Explicit formats, bounded resources, isolated native workers where justified | Parser fuzzing, hangs, memory pressure, unrelated-capture continuity |
| Future feed/episode input | XML external entities disabled, bounded parsing/decompression/downloads, safe HTML rendering, nested URL policy and private-token redaction | Entity/expansion fixtures, malformed feeds, redirected enclosures, changed-object resume, terminal injection and credential leakage |
| External text to terminal | Literal safe rendering and bounded layout work | Control sequences, bidi edge cases, oversized combining sequences, copy behavior |
| Source/model content to planner | Typed proposals, deterministic permission/resource checks | Instructions embedded in transcripts, metadata, packets and model results |
| Client to service | OS peer permissions or authenticated local transport; version and request validation | Unauthorized local clients, replay/idempotency and incompatible versions |
| Service to provider | Destination/type policy, protected credential reference, budget reservation | Local/LAN/hosted routing and billing fault fixtures |
| Import/export to filesystem | Scoped paths, size quotas, no traversal or unsafe link following | Absolute/relative traversal, symlinks/reparse points, archive bombs |
| Extension to host | Explicit trust and capabilities; verified isolation where claimed | Denied access, worker escape assumptions, stalled/oversized messages |
| Release/update to installation | Trusted origin, integrity/signature validation, compatibility and recovery | Substitution, downgrade, interrupted update and restore |

Actual sandbox mechanisms depend on the OS and selected runtime. Process separation improves failure isolation; it does not by itself establish restricted filesystem or network access.

## 3. Network-source policy

Internet station discovery cannot implicitly reach loopback, private networks, link-local services, or local files. Explicit LAN sources and provider endpoints have separately saved destinations and purpose. Check resolved addresses for every connection and validate redirects and nested playlist/segment URLs against the same policy, including IPv6 and alternate representations.

Avoid a validate-then-resolve gap: enforcement must apply to the address actually connected to while retaining correct TLS hostname validation. A decoder that independently opens arbitrary nested URLs can bypass an upstream check. Evaluate constrained fetching, allowed-protocol configuration, or network isolation together with the media backend.

Optional [proxy routing](../design/network-routing.md) requires separate proxy-endpoint and source-destination grants. Delegated DNS changes which addresses Sigy can verify; record that trust explicitly and never label a proxy peer as an observed source peer. Required proxy routes cannot fall back directly. Personal-machine and server deployments share these controls; remote clients need a separately qualified access mechanism.

Keep authentication headers and URL credentials scoped to the intended origin. Do not forward them to a different redirect destination. Logs and error messages redact user information and sensitive query values.

Public HTTP radio can be an explicitly supported source with visible transport status; it must never carry model-provider credentials. Hosted credentialed providers require validated secure transport. Deliberately configured local/LAN exceptions need an explicit profile and documented trust assumptions.

## 4. Data destinations and privacy

Radio audio, source metadata, transcripts, translations, music fingerprints/clips, telemetry, and diagnostics have separate outbound purposes. A profile states exactly what it sends and where. Changing from local transcription to hosted audio analysis changes the data-flow policy, even if both are called transcription.

Record provider data-handling assumptions, configured options, and review date without promising behavior outside Sigy's control. Send only the selected material needed for the task. Disabling remote processing prevents new requests but cannot recall data already transmitted.

No telemetry or diagnostic upload is enabled implicitly by this design. Local diagnostics should report health with bounded retention and redact secrets. User exports can deliberately contain source material; support bundles have a separate minimal manifest.

Publicly accessible reception and unrestricted redistribution are different questions. Store source/usage provenance and evaluate directory, stream, model, dataset, and dependency terms for the actual distribution and export features. This is a release-design task, not a blanket claim about every source's permissions.

Apache License 2.0 is confirmed for Sigy. It does not relicense captured broadcasts, model weights, third-party dependencies, or service data. Preserve their applicable notices and evaluate compatibility for the actual linking, bundling, and distribution arrangement. A process boundary alone does not establish a legal conclusion about that arrangement.

The README carries lawful-use and warranty/liability notices. Do not turn these into claims of universal permission or complete legal protection. Reception, recording, decryption, transmission, and redistribution can require different authorizations. Product profiles retain receive-only hardware defaults and explicit source/provider configuration.

## 5. Credentials and cryptographic keys

Use protected references rather than values in configuration exports, plans, process arguments, or ordinary logs. Identify which service account can access each secret and what happens before login, after logout, and after reboot.

Credential testing separates endpoint availability from a paid operation. Rotating/revoking a key invalidates dependent capabilities predictably. Failure to unlock a key pauses the affected operation without blocking unrelated collection.

Historical demo settings and operational keys have different treatment. The modern operation requirements are in [Signal extensions and workbench](08-signal-extensions-and-workbench.md). Whole-library encryption remains D-23, including search metadata, unlocked state, and backup/recovery decisions.

## 6. Release artifacts and updates

Choose native installers/package-manager paths after selecting supported OS/architecture versions. Record the exact toolchain, dependencies, optional native features, drivers, model assets, licenses, and integrity identifiers for a release. Include a machine-readable component inventory and required third-party notices.

Use trusted distribution and platform-appropriate signing. Evaluate a maintained update framework only if an application-managed updater is needed. An update hash obtained from the same untrusted response as the binary is insufficient origin evidence. [Research basis](../../research/13-security-and-release-engineering.md).

Updates have an explicit active-job policy, migration preflight, backup/restore path, and post-update health check. Never kill captures silently to update a UI. Model/decoder upgrades create new processing provenance. Do not promise binary rollback across an incompatible schema change.

Offline installers, model imports, and dependency bundles must receive equivalent origin/integrity and compatibility checks. Unknown or unsupported assets remain unavailable rather than being silently executed.

## 7. Maintenance and acceptance

Before first release, define supported platforms/dependency versions, advisory monitoring, release and revocation procedures, and a practical vulnerability-reporting route. A public security policy can be written when distribution and maintenance ownership are settled; this planning phase does not invent contacts or guarantees.

Test recovery onto a clean machine using documented artifacts. Include migration failures, full disk, unavailable secret stores, stale update metadata, and a failed model upgrade. Verify that restoring an old library cannot silently resume stale paid requests or old schedules.

First-release acceptance requires network-boundary, local-control, destination, redaction, update/recovery, and dependency checks. Later third-party plugins and modern cryptography require their own focused qualification before enabling those claims. All controls remain independent of the selected application language.
