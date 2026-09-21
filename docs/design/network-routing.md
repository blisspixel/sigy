# User-controlled network routing

Updated: 2026-09-21. Status: planned capability; transport profiles and release placement remain D-34. No proxy configuration or proxy CLI exists yet.

## Purpose and current state

Support a free and open internet by letting users choose how Sigy reaches sources. An explicitly configured proxy can provide an alternative route when a station, directory or feed is unreachable from the user's current network. Preserve open catalogs, direct feeds, offline caches and portable user data alongside that choice. Reachability is conditional; a route is not a guarantee of access, anonymity or an exit country.

Today `sigy-service::sources::HttpAcquirer` disables inherited proxies, checks destination DNS answers and connected peers, and applies explicit redirect policy. Directory requests and audio capture share this boundary. Continue using it; do not add a second HTTP client or give a decoder network access. See [current redirects](../decisions/0007-authorized-redirects.md) and [dated research](../../research/29-network-routing.md).

## Profiles and authority

Proposed profiles distinguish direct connections from a required user-configured proxy. Evaluate HTTP/HTTPS proxies and SOCKS5, including proxy-side DNS. A user-managed OS VPN is compatible in principle with ordinary socket routing, but Sigy does not install it, select its exit or enforce its firewall. Qualify actual behavior before advertising VPN support.

Save immutable route revisions with transport type, endpoint, separately authorized proxy addresses, DNS mode, credential reference and allowed request purposes. Bind each accepted operation to its selected revision. Changing a default affects new operations, not running captures. Disabling a route stops its active transfers and pauses dependent work; unavailable or revoked credentials never select another route.

Source destination grants and proxy endpoint grants are independent. A permitted loopback proxy does not authorize loopback station URLs. The endpoint must have its own checked resolution and connection policy. Source revisions retain their destination and redirect constraints; metadata and model proposals cannot change either grant.

| Request purpose | Proposed route selection |
| --- | --- |
| Catalog refresh and discovery | Explicit discovery profile, including mirror discovery/bootstrap behavior |
| Radio capture and playback | Selected source operation profile; redirects and future playlist/HLS resources inherit it |
| Podcast feed and linked resources | Subscription profile inherited by selected enclosures, transcripts, chapters and artwork; each URL still needs destination validation |
| Model processing | Separate provider profile and data-destination permission; source routing never authorizes transcript/audio uploads |

A required proxy means no automatic direct fallback, including after restart, authentication failure or a redirect. Any eventual fallback list consists only of explicitly authorized profiles and shares the original time, byte, attempt and cost bounds. Keep inherited environment/system proxies, PAC/WPAD and `NO_PROXY` bypasses disabled. Initially prefer one selected route without automatic rotation.

## DNS, peers and transport

DNS policy must be explicit because availability and privacy can depend on where a name is resolved.

| Mode | Enforcement and limits |
| --- | --- |
| Locally checked target resolution | Resolve through Sigy's checked resolver, select an allowed address and send that numeric target through the tunnel while retaining the original HTTP host and TLS identity. Prove this behavior in the connector. Local DNS requests remain visible to the configured resolver. |
| Delegated target resolution | Send the hostname to the proxy without local target lookups. Sigy validates URL policy but cannot independently prove the proxy's selected target IP. Requires an explicit trusted-proxy policy and a decision on proxy-side destination enforcement before implementation; never label the result locally IP-verified. |

For either mode, the proxy is a trusted intermediary for forwarding. Checking the requested numeric target does not independently prove where a dishonest proxy connected. Source TLS validation must remain enabled. An HTTP source remains readable and mutable by intermediaries even when the client-to-proxy link is encrypted. Protect remote proxy authentication with a qualified secure transport or user-managed secure tunnel; plain SOCKS username/password authentication is not encryption.

Proxy bootstrap is separate from target DNS. A hostname proxy still needs resolution unless its address is pinned. A profile that promises no direct DNS must account for bootstrap, directory SRV discovery and all linked resources. Unsupported discovery must fail visibly or use an explicitly configured mirror through the same route, never silently issue direct discovery requests.

The current `HttpHop.peer` describes a direct connection to the source. Introduce a versioned observation before enabling proxies: route revision, observed proxy peer, requested target, DNS mode and the basis of any target-address claim. A proxy socket peer or SOCKS bound address is not the destination server's observed peer. Retain old direct-route evidence without reinterpretation. Secrets, userinfo and sensitive URL paths/queries remain excluded from ordinary route diagnostics and sidecars.

## Experience, resources and costs

CLI and TUI use the same service operations for route configuration, selection, status and explicit bounded diagnostics. Display direct/proxy mode, the selected profile, DNS mode and actionable failure causes. Keep credential values out of command arguments, ordinary configuration exports, logs and terminal text. Use the protected credential mechanism planned for the persistent service, including locked-store behavior before login.

Keep station origin, receiver location, user-configured exit label and any measured exit observation separate. Do not infer station location from a proxy or use an exit label as evidence of geography. Do not contact a third-party IP/geolocation checker automatically. Report an observed 403, 451, timeout or proxy failure without declaring its cause to be geographic blocking unless evidence supports that conclusion.

Reuse bounded attempts, deadlines, cancellation, backpressure and storage reservations. A proxy handshake or authentication retry consumes the same operation allowance. Preserve capture gaps and route changes in provenance. Cached discovery and retained media remain available when the network route fails.

Users supply their own endpoints. Do not buy service, discover public proxy lists or create recurring subscriptions. Metered proxy integrations need a qualified worst-case pricing bound, ledger reservation and provider-side cap where client byte accounting cannot bound billing. Existing credentials do not authorize spending. An external VPN or proxy bill is outside Sigy's accounting unless explicitly integrated; avoid promising a universal billing cap.

## Delivery and evidence

1. Evaluate the existing pinned HTTP stack with local proxy fixtures. Compare locally checked SOCKS5 targets with delegated-DNS and HTTP CONNECT behavior. Review enabled features and the complete dependency graph before adding a package.
2. Record D-34 with the qualified transport/DNS subset. Implement route revisions, purpose binding, secret references, revocation and provenance migration through existing service/storage operations. Keep unsupported modes unavailable.
3. Expose complete CLI operations and then TUI controls. Qualify discovery, capture and podcast-linked requests together; test the process after client exit and service restart.

Acceptance R-58 requires fixture counters proving no direct target connection or unexpected DNS request; environment bypass rejection; public/private and mixed-address checks; independent proxy/target pins; IPv4/IPv6; original hostname TLS verification through a tunnel; bad certificates; auth isolation and redaction; redirects and nested resources; stale profiles, revocation and restart; unavailable proxies; bounded stalled handshakes, cancellation and retries; and truthful versioned route metadata. Use local synthetic media and proxy servers without paid services. Later real-network evidence must name the tested platform and profile, not claim universal reachability.

Protocol availability, delegated-DNS trust, credential storage and the release milestone remain open. This plan does not expand the first complete release promise before those gates are resolved.
