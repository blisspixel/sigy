# Redirects at the radio acquisition boundary

Reviewed: 2026-09-21. Scope: the existing Rust HTTP stack, without a new transport dependency.

HTTP defines redirect status semantics and URI references in Location fields, including relative references. Clients should detect cycles. For Sigy's GET-only acquisition, 301, 302, 303, 307 and 308 can lead to another GET; other 3xx statuses do not automatically identify a playable response. Sigy deliberately rejects fragments, ambiguous Location fields and insecure downgrades as profile constraints. [RFC 9110, redirect semantics](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.4), [Location](https://www.rfc-editor.org/rfc/rfc9110.html#section-10.2.2).

Reqwest provides automatic redirect policies, including disabling redirects. Keep that automatic behavior disabled and use the existing checked HTTP seam for each authorized hop. This allows validation before connection and preserves explicit DNS/peer checks, shared concurrency and one overall deadline. The pinned 0.13.5 API compiles in the workspace; no package upgrade is needed. [Maintainer documentation](https://docs.rs/reqwest/latest/reqwest/redirect/index.html).

Alternatives considered: continuing to reject all redirects prevents useful station compatibility; enabling a general library redirect limit does not express Sigy's source grants or retained provenance. A bounded manual loop reuses URL parsing and connection security rather than duplicating them. Never delegate network URL resolution to the media decoder, whose current input contract is a local file.

The selected profile separates deny, same-origin and public-internet permission. A public redirect is not permission to contact private addresses; an address pin cannot become authority for a different origin. Drop response bodies between hops and retain no cookies, credentials or referer state. Store bounded origin/peer/status observations with successful publication; neither a 200 status nor a MIME label proves playable media. Decoder validation remains required.

Qualification uses local servers for relative redirects, supported statuses, loops, excessive chains, duplicate/missing Location, denied destinations, unchanged byte limits and stop during headers. Pure policy cases cover normalized private literal addresses, scheme downgrade, credentials and fragments; existing DNS/TLS tests remain applicable. A decoder-backed CLI fixture verifies actual media bytes and route metadata together. These tests do not establish broad public-radio interoperability or Unix behavior.
