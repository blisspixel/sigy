# Explicit proxies and network reachability

Reviewed: 2026-09-21. Scope: user-controlled radio/podcast routing in the existing Rust stack. Status: primary-source review and local source inspection, not proxy interoperability testing.

## Current evidence

The workspace pins reqwest 0.13.5 with default features disabled and `rustls` enabled. `HttpAcquirer::open_once` explicitly calls `no_proxy()`, installs `CheckedResolver`, and validates the connected peer against the source grant. The resolver's pinned-address mode applies to every lookup on that client. Adding a proxy without separating proxy and destination resolution would therefore alter a security invariant, not merely add a URL option. Recording envelope v2 also assumes a directly observed source peer.

The maintainer's versioned API supports proxy selection for HTTP, HTTPS or both, proxy authentication and exclusions. The HTTP/HTTPS selectors describe the destination URL scheme; the proxy URL separately describes the connection to the proxy. SOCKS support is feature-gated. No dependency upgrade is indicated merely to evaluate explicit proxies. [Reqwest 0.13.5 Proxy](https://docs.rs/reqwest/0.13.5/reqwest/struct.Proxy.html), [ClientBuilder](https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html#method.no_proxy).

Inspection of the downloaded 0.13.5 package's `src/connect.rs` confirms that `socks5` uses local target resolution and `socks5h` delegates it to the proxy. The local branch obtains a resolved target and passes a numeric address onward; the Rustls branch retains the original destination hostname for TLS. This is useful implementation evidence, but does not prove Sigy's pinning, proxy-peer or cancellation behavior. The versioned web source page was unavailable during review; the published API and installed package were inspected instead. Recheck this code when the dependency changes.

HTTP CONNECT asks an intermediary to establish a tunnel to a host and port. It does not supply independent proof of the remote endpoint's IP. A proxy can itself forward through another proxy. Design inference: direct-peer observations cannot be reused unchanged for tunneled connections. [RFC 9110 section 9.3.6](https://www.rfc-editor.org/rfc/rfc9110.html#section-9.3.6).

SOCKS5 distinguishes numeric IPv4/IPv6 targets from domain-name targets. Its successful CONNECT reply contains the server's bound address and port, not an independently observed destination peer. Basic SOCKS username/password authentication sends the password without confidentiality protection. Design inference: keep requested destination, observed proxy peer and delegated claims distinct, and require an appropriate protected channel for remote credentials. [RFC 1928 sections 4-6](https://www.rfc-editor.org/rfc/rfc1928.html#section-4), [RFC 1929 section 3](https://www.rfc-editor.org/rfc/rfc1929.html#section-3).

## Alternatives and recommendation

| Approach | Benefit | Limitation and recommendation |
| --- | --- | --- |
| User-managed OS VPN | Works below application sockets, with no VPN implementation in Sigy | Routing, DNS and disconnect protection depend on external configuration. Qualify compatibility; do not claim Sigy enforces a kill switch. |
| Explicit SOCKS5 with local target resolution | Candidate for preserving checked destination addresses through a tunnel | Local target DNS can itself be blocked or observable. Requires separate proxy resolution and accurate route evidence. Evaluate first. |
| SOCKS5 with delegated DNS | Can resolve names from the proxy's network without local target lookups | Target IP policy moves to the trusted proxy. Explicitly model that trust and bootstrap DNS before enabling it. |
| HTTP/HTTPS proxy | Common user-supplied infrastructure; can tunnel HTTPS | Forwarding and CONNECT can delegate target resolution. HTTP source content is not end-to-end encrypted. Qualify DNS, authentication and target policy per mode. |
| Embedded VPN or automatic proxy marketplace | More integrated route management | Adds privileged networking, packaging, account and billing scope. No present need justifies it. |

Recommend a shared route profile alongside source authorization, with no implicit environment inheritance or direct fallback. Bind discovery, source media and linked resources to explicit purposes; model-provider routing remains separate. Supporting alternative routes advances user choice and access to information, without requiring a central Sigy relay or collecting users' traffic.

## Qualification still needed

Use bounded local proxy/DNS/TLS fixtures to establish exact connection targets, resolver invocations, original-host certificate checks, redirect behavior, credential isolation and cancellation. Check numeric literals as well as resolved hostnames. Inspect feature-level dependency changes rather than assuming SOCKS requires another library. Test disabled or unavailable profiles across persistent-service restarts and retain honest route metadata.

Proxy billing may include connection overhead, minimum charges or traffic that application body counters do not measure. No paid endpoint was used or selected in this review. Apply the existing cost gate before any metered integration; do not infer a dollar ceiling from a media byte limit alone.

The [routing design](../docs/design/network-routing.md) records the proposed contract. D-34 selects the supported transport and trust subset; R-58 defines acceptance. No claim of censorship resistance, anonymity, geographic availability or platform support has been measured.
