# Historical and modern cryptography

Reviewed: 2026-09-22. Status: research and proposed contracts for future roadmap stages 11 and 12. No cryptographic implementation or provider has been selected.

## Confirmed intent

Sigy should be useful and enjoyable to explore. A historical cipher laboratory, including Enigma with a visual TUI, lets users generate their own encrypted messages, inspect them, recover plaintext with supplied settings or a bounded classical challenge, and reveal, compare, and replay the result. Modern authenticated encryption and post-quantum cryptography using supplied keys are also confirmed requirements, with separate offline lessons using demonstration keys.

These capabilities share versioned inputs, outputs, and replay with the wider platform, while preserving distinct purposes and key-handling rules. Enigma demonstrations are not candidates for protecting the Sigy library.

## Historical evidence

Enigma machines vary in rotor arrangements and stepping behavior. Correct reproduction requires a named machine variant, rotor order/wiring, ring settings, initial positions, reflector, and applicable plugboard behavior. Double stepping is a specific behavior to verify. [Crypto Museum's mechanism description](https://www.cryptomuseum.com/crypto/enigma/working.htm).

Museum-derived rotor specifications provide an independent reference for wiring and turnover data. A future fixture set must state position conventions, forward/reverse traversal, and stepping order instead of assuming every simulator uses the same notation. [Rotor specifications](https://www.codesandciphers.org.uk/enigma/rotorspec.htm).

Proposed initial historical scope: a fully specified Enigma I simulation, with room for other documented variants and simpler substitution/polyalphabetic ciphers. Caesar and Vigenere are candidates for approachable lessons; their exact alphabet and normalization contracts still need a focused specification. They are not implied to preserve arbitrary Unicode text.

### Challenge evidence

The [laboratory flow](../docs/planning/08-signal-extensions-and-workbench.md#offline-cipher-laboratory) separates generating the exercise, solving it, and revealing its answer. Preserve the original input, explicit alphabet conversion, ciphertext, settings, implementation version, released cribs, search bounds, coverage, and candidate history. The solution record and any revealing generation seed stay outside solver-visible artifacts, prompts, logs, and traces. A hidden UI panel alone is not an isolation boundary.

Known-settings decryption demonstrates the specified transform. Unknown-key historical search explores a declared finite domain or bounded heuristic strategy. Neither a plausible sentence nor a high score proves exact recovery. After the attempt is fixed, compare with the declared challenge plaintext and distinguish exact plaintext recovery from original-key recovery or equivalent settings. Interrupted or incomplete search retains its stopping reason and cannot claim the whole domain was ruled out.

This scope is self-generated offline fixtures and classical methods, without radio transmission. Planned setup defaults to United States guidance and asks for country and applicable state/region, stored locally and editable without GPS or IP inference. A selection does not grant legal authority. Keep notices brief: educational/general information rather than legal advice, authorized material only, and the existing [Apache License 2.0](../LICENSE) warranty terms. Later non-fixture use and distribution need assessment under the applicable jurisdiction. [Shared legal and privacy design](../docs/planning/09-security-privacy-and-release.md).

For one specific US boundary, 47 CFR 97.113(a)(4) restricts amateur-station messages encoded to obscure meaning, subject to its stated exceptions. This is an amateur-radio rule, not a statement about all radio services or offline cipher exercises. [Current FCC rule](https://www.ecfr.gov/current/title-47/chapter-I/subchapter-D/part-97/subpart-B/section-97.113).

## Modern evidence

NIST's current PQC project identifies ML-KEM, ML-DSA, and SLH-DSA as finalized standards while additional candidates remain under evaluation. Lessons must distinguish standardized profiles from research candidates and current implementation evidence. [PQC project](https://csrc.nist.gov/projects/post-quantum-cryptography), [current NIST overview](https://www.nist.gov/pqc).

| Capability | Evidence | Product consequence |
| --- | --- | --- |
| Authenticated encryption with associated data | Maintained libraries expose AEAD operations with key, nonce, ciphertext, authentication tag, and associated-data requirements | Use vetted implementations and exact algorithm limits; never design a primitive for the TUI; [libsodium AEAD](https://libsodium.gitbook.io/doc/secret-key_cryptography/aead) |
| Post-quantum key establishment | FIPS 203 defines ML-KEM | A KEM establishes key material; payload protection needs an appropriate symmetric construction and protocol; [FIPS 203](https://csrc.nist.gov/pubs/fips/203/final) |
| Post-quantum signatures | FIPS 204 defines ML-DSA; FIPS 205 defines SLH-DSA | Signing/verifying and encryption/decryption require separate capabilities; [FIPS 204](https://csrc.nist.gov/pubs/fips/204/final), [FIPS 205](https://csrc.nist.gov/pubs/fips/205/final) |
| Native implementation candidate | OpenSSL 3.5 documents ML-KEM, ML-DSA, and SLH-DSA in its default provider | Evaluate a concrete maintained implementation and bindings; availability is not a Sigy support or certification claim; [default provider](https://docs.openssl.org/3.5/man7/OSSL_PROVIDER-default/) |
| Experimental algorithm breadth | liboqs explicitly positions itself for research/prototyping and advises against relying on it for production protection of sensitive data | Keep exploratory algorithms outside the production protection path; [upstream limitations](https://github.com/open-quantum-safe/liboqs#limitations-and-security) |

The FIPS 203 page carries a November 2025 potential-update note, and the FIPS 204 page carries a July 2026 potential-update note. Standards, errata, library advisories, test vectors, and exact implementation versions must be reviewed together at selection. An algorithm standardized in a FIPS document does not make every implementation a validated cryptographic module.

Post-quantum means designed to resist relevant quantum attacks under its security assumptions. It does not mean immunity to implementation flaws, stolen keys, or all future attacks. Sigy's capability labels must identify the actual algorithm, parameter set, operation, and implementation.

An offline ML-KEM lesson should demonstrate encapsulation/decapsulation to a shared secret, followed by a reviewed key-derivation and AEAD profile for the payload. A KEM ciphertext is distinct from the encrypted message, and a KEM alone does not authenticate the sender. ML-DSA and SLH-DSA lessons demonstrate signature creation, verification, and tamper rejection separately. Use maintained implementations, current errata, and independent vectors; successful round trips demonstrate execution, not a new proof of quantum resistance. Quantum breaking and unknown-key attacks on modern schemes are outside the proposed laboratory. [NIST KEM guidance](https://csrc.nist.gov/pubs/sp/800/227/final).

## Proposed capability design

Separate historical transform, authenticated encrypt/decrypt, sign/verify, and encapsulate/decapsulate operations. A generic transform-by-name interface cannot adequately express their different inputs, outputs, failure states, and key requirements.

Versioned profiles bind algorithm identifiers, parameters, key encoding, nonce rules, associated data, payload format, and verification semantics. Key references are separate from public metadata. Unsupported or deprecated profiles fail explicitly; fallback cannot silently change protection.

Protocol adapters keep their own documented cryptographic framing. A generic workbench does not replace Meshtastic's security or make arbitrary encrypted captures readable. Decryption requires a supported scheme, correct framing/parameters, and appropriate key material. Unknown payloads remain inspectable opaque data.

Large files require an evaluated streaming/container construction with sequence, truncation, and reordering protection. If chunks are individually authenticated, show that status distinctly from verification of an entire completed artifact. Do not invent a new secure file format as an incidental UI feature.

## Key lifecycle and display

- Operational keys remain local unless an explicitly configured key service is part of a later reviewed design. They never enter model prompts, normal logs, command history, or ordinary evidence exports.
- Use scoped secret references and protected key import. Evaluate OS secret storage, unattended service access, backup, recovery, rotation, and deletion together.
- A workbench trace may expose historical demonstration settings. Operational views show algorithm, public fingerprint, operation, and authentication result, without displaying secret intermediate state.
- Use library-provided secure randomness and documented key/nonce limits. Test nonce management across concurrency, restart, and retries.
- Do not release unauthenticated plaintext as a successful result. Verification failure and missing key are distinct from unavailable decoder and malformed input.
- Best-effort secret erasure must account for FFI copies, runtime behavior, crash dumps, and diagnostic collection. No memory-safety language alone proves key secrecy.

These are product design constraints, not claims that key handling has been implemented or assessed.

## Fun and understandable interaction

An Enigma session should let the user choose a documented configuration, type a message, step one character, watch rotor positions and the signal path, replay, compare settings, and save a demonstration. A compact textual trace must provide the same information when animations are disabled.

A modern workbench can visualize which artifacts move through encryption, authentication, key establishment, or signature verification. Clearly show tamper failures and algorithm identities. It should teach the actual operation without implying that attractive animation demonstrates security.

Morse practice, packet inspection, and cipher experiments can share a replay timeline and saved workspace. Synthetic exercises retain synthetic provenance and cannot silently enter a real-source monitoring report.

## Alternatives and decisions still open

Evaluate maintained native cryptographic libraries against any language-native implementations after application requirements and support targets are fixed. Consider maintenance, advisory response, independent review, API misuse resistance, key storage integration, platform packaging, and test-vector coverage.

Modern archive encryption, transport encryption, and workbench transformations are separate product decisions. Supporting a workbench operation does not automatically establish whole-library encryption at rest. Hybrid key establishment should use a reviewed protocol construction if adopted; do not combine primitives ad hoc.

Post-quantum key and signature sizes matter for small radios. Do not assume an operation that works in a desktop file workflow fits a constrained packet transport. Measure framing overhead and follow the actual radio protocol.

## Future acceptance evidence

Historical conformance requires independent known-answer fixtures, rotor transitions including double stepping, reset/replay determinism, alphabet handling, and agreement between visible trace and transform result. Challenge tests must cover false cribs, equivalent keys, ambiguous or wrong candidates, cancellation, exhausted bounds, and hidden-answer leakage. With the public challenge and solver configuration fixed, changing only the withheld answer must not change solver work or candidates; only the later comparison may change. Round trips alone cannot establish compatibility.

Modern validation requires official or upstream known-answer vectors, interoperability, malformed inputs, wrong key/tag/associated data, truncation/reordering, restart/concurrent nonce cases, secret redaction, and dependency/advisory review. Performance and footprint must be measured on both host classes. Security-sensitive integration needs focused review before a production protection claim.

These capabilities remain future roadmap stages 11 and 12. The current language pipeline and first-release build order continue unchanged; the laboratory requires its own implementation and qualification evidence.
