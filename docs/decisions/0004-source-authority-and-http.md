# 0004: Immutable source authority and finite HTTP transport

Date: 2026-09-21. Status: source registration and transport implemented with local Windows tests. This records the initial transport slice. [Recording and retention](0005-recording-and-retention.md) subsequently added asynchronous DNS, executable finite recording, media publication and IPC v4; earlier limitations below are historical.

## Source configuration

Schema v3 adds immutable `http_audio` source revisions to the existing catalog.
An explicit revision key identifies the complete normalized URL, display name
and network scope. An exact replay returns the same revision. Any change needs
a new key. Registration does not resolve DNS, contact a station or record data.
CLI `source add`, `source list` and `source show` use the same operations offline
and through the running controller. IPC v3 rejects older clients.

The catalog admits at most 4,096 revisions atomically and serves pages of 1
through 32 entries. SQL constraints and immutable-row triggers protect ordinary
mutations; opening and reading reconstruct validated configurations. Migration
from v1/v2 is transactional. Existing capture references are not retroactively
authorized or assigned a fabricated source. Dispatch must resolve the exact
registered revision and check current permission before starting an attempt.

Names retain original Unicode scripts. Control characters, line separators and
directional override/isolation controls are rejected in names. URL parsing
rejects credentials in userinfo, fragments, whitespace, unsupported schemes and
unsafe literal destinations. Ordinary views and debug output omit URL paths and
queries. Full URLs are stored in plaintext in the private library, not a secret
vault. Do not place access credentials in source URLs; secret references and
authenticated-source support require a separate credential integration.

## Network authority

Public-internet scope accepts a conservative subset of ordinary unicast. It
rejects private, loopback, link-local, multicast, mapped/transition, documentation
and reserved addresses, plus the Azure host-platform virtual address. Some
globally reachable special-purpose allocations are intentionally excluded.

An explicit `--pin-address` stores one public, RFC1918, IPv6 unique-local or
loopback address for that revision. A hostname then connects to that exact IP
without a DNS lookup while retaining the hostname for HTTP and TLS. A literal
URL must match the pin. This is not a blanket LAN grant. Link-local and other
unsupported special addresses remain denied. Future station discovery and model
proposals must not manufacture this user-controlled permission.

The custom resolver checks every returned address before handing it to the HTTP
connector. Mixed public/private results, empty results and more than 16 results
fail closed. It does not validate and then perform a second unchecked lookup.
Implicit proxies are disabled, and the connected peer is checked again. All
redirects are rejected in this profile, including same-origin redirects. There
is no secondary resource fetching or automatic retry. Network routing, custom
NAT and a compromised host remain outside this application-level IP policy.

## Finite transport

`sources::http::HttpAcquirer` is the canonical HTTP acquisition seam. One shared
instance admits at most two attempts and two system-DNS tasks. Each call requires
positive byte and elapsed-time limits, capped at 256 MiB and 15 minutes. These
are conservative admission limits, not measured recording capacity. Reads stream
to a caller-owned sink without collecting the full body. The elapsed deadline
includes setup, body reads and sink writes; connection/read waits also have a
five-second ceiling. The current Hyper HTTP/1 parser has a finite buffer limit;
Sigy also caps response header count at 64 and tests oversized headers.

HTTP 200 and a supported declared audio content type are required. Redirects,
playlists, encoded HTTP bodies and ambiguous content types fail before body
writes. Interleaved ICY metadata also fails before a body write unless the
recording explicitly requests it. See [ICY observations](0013-icy-observations.md).
TLS uses offline WebPKI verification with explicit Mozilla roots from
`webpki-root-certs` 1.0.9, preserving chain, validity and hostname checks. Platform verification can retrieve certificate-supplied URLs
outside the source policy, so this adapter deliberately does not use it. A
loopback fixture tests rejection of a self-signed certificate. Bundled roots
require timely dependency updates; OS-local enterprise roots and online
revocation retrieval are not inherited. Additional trust profiles require an
explicit policy and qualification. This does not establish successful
real-station playback.

A transfer receipt reports bytes accepted by the sink, peer address, declared
content type and either body EOF or the byte limit. It is not a checksum,
decoded-media result, durable-write receipt or completed capture. Timeouts,
transport errors and sink failures return errors; partial bytes remain
unverified. A sink may have unfinished I/O after cancellation. The future media
owner must drain or terminate its work before finalization or reuse.

## Remaining integration gates

System DNS uses a bounded blocking task. Cancellation retains its semaphore
slot until the OS lookup actually returns, preventing repeated timeouts from
creating unbounded DNS work. Such a task can still delay runtime shutdown.
Resolve this through a qualified cancellable resolver or supervised process
before connecting automatic recording to the long-lived service. Do not claim
that an async request deadline terminates an OS call.

The next media slice must add physical storage reservation, safe file ownership,
source permission revocation, bounded worker shutdown, codec validation,
checksums, timing/completeness, durable publication and orphan reconciliation.
Only then may it expose recording commands and capture completion. Longer radio
formats, redirects, metadata and authenticated streams need explicit qualified
extensions to this same boundary. Other signal kinds get typed configurations;
the HTTP-audio profile does not become a generic assumption that all signals are
audio.

Dependency alternatives, current versions and primary evidence are in
[HTTP acquisition research](../../research/25-http-acquisition.md). Acceptance
evidence and the next bounded task live in [active work](../development/progress.md).
