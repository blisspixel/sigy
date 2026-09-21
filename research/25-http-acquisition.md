# HTTP source acquisition

Reviewed: 2026-09-21. Implemented scope is defined by
[decision 0004](../docs/decisions/0004-source-authority-and-http.md).

## Dependency choice

The current registry and primary documentation identify Reqwest 0.13.5 and
Ureq 3.4.2 as stable releases. Reqwest supports async streaming, a custom
pre-connection resolver, disabled proxy discovery, explicit redirect policy and
disabled retry policy. These fit the existing Tokio service. Select Reqwest with
default features off and `rustls` only. Do not enable cookies, decompression,
HTTP/2, HTTP/3 or proxy discovery implicitly. See its
[versioned builder API](https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html),
[resolver contract](https://docs.rs/reqwest/0.13.5/reqwest/dns/trait.Resolve.html)
and [retry policy](https://docs.rs/reqwest/0.13.5/reqwest/retry/fn.never.html).

Ureq is a smaller blocking alternative, with configurable global timeouts and
proxy/redirect behavior. Its custom resolver API is explicitly unversioned and
its default timed DNS resolution spawns a thread. A blocking adapter also needs
owned worker and shutdown machinery. Those tradeoffs outweigh its lower runtime
surface for this service. Evidence:
[configuration](https://docs.rs/ureq/3.4.2/ureq/config/struct.ConfigBuilder.html)
and [resolver source](https://docs.rs/ureq/3.4.2/src/ureq/unversioned/resolver.rs.html).
Handwritten HTTP/TLS or letting a decoder fetch arbitrary URLs would duplicate
security-sensitive protocol behavior and bypass the canonical source policy.

The selected TLS graph includes Rustls 0.23.45, platform-verifier 0.7.0,
AWS-LC-RS 1.18.1 and AWS-LC-SYS 0.45.0. This adds native cryptographic build inputs
and platform certificate APIs in the dependency graph; Rust application code
alone is not a pure-Rust distribution. Tokio-Rustls 0.26.5 is also a direct development dependency for a
local TLS rejection fixture, reusing the resolved TLS graph. Review native
build requirements and notices before packaging. The relevant primary sources
are [Rustls](https://docs.rs/rustls/0.23.45/rustls/),
[platform verifier](https://docs.rs/rustls-platform-verifier/0.7.0/rustls_platform_verifier/)
and [AWS-LC-RS](https://docs.rs/aws-lc-rs/1.18.1/aws_lc_rs/).

The source adapter explicitly selects offline WebPKI verification through
Reqwest's `tls_certs_only`, using Mozilla roots from `webpki-root-certs` 1.0.9.
The default platform verifier may perform certificate-driven network retrieval;
its Windows implementation does not request cache-only validation. The Windows
[certificate-chain API](https://learn.microsoft.com/en-us/windows/win32/api/wincrypt/nf-wincrypt-certgetcertificatechain)
documents the flags needed to suppress such retrieval. Uncontrolled AIA/OCSP/CRL
requests would bypass source destination checks. Bundled roots avoid that path
but require prompt trust-store updates and do not inherit enterprise roots or
online revocation data. See the
[root package](https://crates.io/crates/webpki-root-certs/1.0.9).
Some versioned documentation pages were unavailable to the web reader; the
downloaded registry source and manifests were inspected directly as fallback.

The lockfile now contains 182 audited dependency entries, including target-only
and optional resolution entries. That is not the number compiled into a Windows
binary. The all-target active graph contains two versions of Base64 and Syn due
to upstream compatibility ranges. Repeated same-version host/target build units
are not additional version conflicts. Do not rewrite transitive constraints to
hide duplication. Internationalized host parsing includes URL 2.5.8, IDNA and
ICU dependencies. Disabling that behavior merely to lower a package count would
conflict with multilingual sources.

## Destination and resource boundaries

The [IANA IPv4 special-purpose registry](https://www.iana.org/assignments/iana-ipv4-special-registry)
and [IPv6 registry](https://www.iana.org/assignments/iana-ipv6-special-registry)
were reviewed for the conservative unicast policy. A global-looking address is
not by itself sufficient: Microsoft documents
[168.63.129.16 as an Azure host-platform virtual address](https://learn.microsoft.com/en-us/azure/virtual-network/what-is-ip-address-168-63-129-16).
Sigy excludes it from source traffic. Some legitimate special-purpose public
addresses remain intentionally unsupported. Registry changes require policy
review and regression tests; local network translation still needs deployment
controls beyond URL validation.

Reqwest's request timeout and Sigy's outer deadline bound the awaited transfer,
including a stalled sink. They do not establish hard process termination.
[Tokio documents](https://docs.rs/tokio/1.53.1/tokio/task/fn.spawn_blocking.html)
that started blocking tasks cannot be aborted and may delay runtime shutdown.
The current DNS adapter retains a finite permit inside each blocking task.
Qualify a cancellable resolver or supervised process before service dispatch.

The locked Hyper 1.11.1 HTTP/1 implementation limits its adaptive read buffer to
417,792 bytes by default. Sigy tests rejection of a 512 KiB header and caps header
count separately. This is version-sensitive behavior, not a universal maximum
resident-memory claim. Recheck its
[buffer implementation](https://docs.rs/crate/hyper/1.11.1/source/src/proto/h1/io.rs)
when upgrading. TLS, OS networking, DNS and caller-owned sinks have additional
memory and lifetime costs.

## Media boundary still open

Symphonia 0.6.1 is a current pure-Rust media candidate with selectable formats
and codecs, MPL-2.0 licensing and a Rust 1.85 minimum. Version 0.6 introduced API
changes, so older examples need review. See the
[versioned crate](https://docs.rs/crate/symphonia/0.6.1) and
[upstream releases](https://github.com/pdeljanov/Symphonia/releases).
It is not an application dependency yet. A supervised FFmpeg/FFprobe adapter
remains an alternative, provided it receives retained local bytes and cannot
open unapproved protocols or nested network resources. Neither a MIME label nor
a successful HTTP transfer proves that either decoder can safely interpret the
body. Selection needs bounded malformed-media fixtures and process/resource
evidence before completed recordings are exposed.

The current tests use deterministic local HTTP/TLS fixtures. They spend no
provider funds and do not establish broad live-radio compatibility, throughput,
language quality or native Linux/macOS support.
