# Security, privacy, and release engineering

Reviewed: 2026-09-20. Status: research supporting proposed engineering controls; no implementation security assessment has occurred.

## Evidence

NIST's Secure Software Development Framework describes practices that can be integrated into a development lifecycle, including reducing vulnerabilities and addressing their causes. Use it as a reference for maintainable engineering evidence, not as a certification claim or a requirement to adopt a particular process tool. [SP 800-218](https://csrc.nist.gov/pubs/sp/800/218/final).

The Update Framework documents rollback, freeze, repository compromise, and related update threats. A checksum proves consistency with a supplied digest, not that the supplier or update is authorized. Update design needs trust metadata and recovery rules as well as artifact integrity. TUF is an approach to evaluate if Sigy operates its own updater; native package-manager distribution is an alternative. [TUF security design](https://theupdateframework.io/docs/security/), [specification index](https://theupdateframework.io/spec/).

FFmpeg exposes protocol restrictions and network timeout options. They are useful controls around media inputs, but Sigy must still govern destinations, nested playlists, credentials, and worker permissions. [Protocol documentation](https://ffmpeg.org/ffmpeg-protocols.html).

rtl_433 explicitly warns that radio-derived content is untrusted. Similar boundaries apply to directory metadata, decoded packets, and speech transcripts even when reception itself is authorized. [Upstream security guidance](https://github.com/merbanan/rtl_433#security).

Provider destination, routing, and billing controls are researched separately in [Model providers and costs](04-model-providers-and-costs.md). Cryptographic operation and key-lifecycle considerations are in [Cryptography](12-cryptography.md).

## Design implications

Sigy fetches user-selected URLs and automatically discovers sources. A malicious source can therefore target the host through redirects, nested resources, decoder inputs, or model instructions. Destination enforcement must cover every network open, not just the first URL supplied by the CLI.

Separate explicitly configured LAN model/device endpoints from discovered internet content. Blocking all private addresses globally would break a confirmed feature; letting every source reach private services would undermine isolation. Use purpose-specific policies and ensure decoders cannot bypass them through nested resource loads.

Local default processing is also a data-flow claim. Define which original audio, text, metadata, fingerprints, or diagnostics can leave the host for each feature. Endpoint location alone is insufficient when a runtime can route inference to a hosted service.

Native dependencies and model assets are part of the delivered product. Record source, version, integrity, license, enabled codecs/features, acceleration requirements, and maintenance status. A single executable does not remove those responsibilities.

## Alternatives and evaluation

Compare package-manager updates, signed native installers, and any application-managed updater against platform coverage, offline installation, trust/key recovery, interruption behavior, and long-term maintenance. Do not select a custom updater before establishing a need.

Compare OS-protected credentials, runtime secret handles, and unattended key-unlock strategies by actual service mode. Desktop convenience and boot-time unattended access have different requirements. A backup plan must preserve recoverability without accidentally exporting operational secrets into ordinary diagnostic bundles.

Future verification needs malicious playlist/redirect fixtures, DNS/address-policy cases, parser fuzzing, terminal-control injection, import traversal, secret-redaction cases, compromised-update fixtures, and interrupted installation/migration tests. Enforce identical application policies through CLI, TUI, and later clients.

## Licensing and lawful-use documentation

Apache License 2.0 is the confirmed project license. The root LICENSE preserves its text, including the warranty and liability provisions. Third-party code, weights, codecs, data, and collected content need their own license and distribution review. A separate process is not an automatic exemption from upstream obligations. [License text](https://www.apache.org/licenses/LICENSE-2.0.txt), [license FAQ](https://www.apache.org/foundation/license-faq.html).

Reception, recording, disclosure, decryption, transmission, privacy, and copyright can involve different permissions depending on jurisdiction and use. As one jurisdiction-specific example, U.S. federal law addresses communications interception/disclosure with defined conditions and exceptions. This is not a global legal clearance or a claim that every radio activity has the same restriction. [47 U.S.C. 605](https://uscode.house.gov/view.xhtml?req=%28title%3A47+section%3A605+edition%3Aprelim%29).

The README states intended lawful use, user responsibilities, output fallibility, and the license's warranty/liability terms. It does not claim that public accessibility establishes all reuse rights, or that a disclaimer changes applicable law. Revisit relevant jurisdiction/device/provider questions before shipping a specific integration.

## Maintenance review

Recheck dependency advisories, signing and OS distribution requirements, provider data handling, and standards updates before release. Keep a supported-version policy, documented vulnerability-response route, and tested upgrade/restore process. No software-quality label substitutes for those maintained practices.
