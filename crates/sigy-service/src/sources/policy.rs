use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

// Conservative subset of ordinary unicast. Special-purpose allocations remain
// denied even when an IANA exception is globally reachable. Review dated in ADR.
const BLOCKED_V4: &[(u32, u32)] = &[
    (0x0000_0000, 8),
    (0x0a00_0000, 8),
    (0x6440_0000, 10),
    (0x7f00_0000, 8),
    (0xa9fe_0000, 16),
    // Azure host-platform virtual address, despite its public IPv4 allocation.
    (0xa83f_8110, 32),
    (0xac10_0000, 12),
    (0xc000_0000, 24),
    (0xc000_0200, 24),
    (0xc01f_c400, 24),
    (0xc034_c100, 24),
    (0xc058_6300, 24),
    (0xc0a8_0000, 16),
    (0xc0af_3000, 24),
    (0xc612_0000, 15),
    (0xc633_6400, 24),
    (0xcb00_7100, 24),
    (0xe000_0000, 3),
];

pub(super) fn is_public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => public_v4(address),
        IpAddr::V6(address) => public_v6(address),
    }
}

fn public_v4(address: Ipv4Addr) -> bool {
    let value = u32::from(address);
    !BLOCKED_V4
        .iter()
        .any(|&(network, prefix)| value >> (32 - prefix) == network >> (32 - prefix))
}

fn public_v6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    segments[0] & 0xe000 == 0x2000
        && !(segments[0] == 0x2001 && segments[1] < 0x0200)
        && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
        && segments[0] != 0x2002
        && !(segments[0] == 0x2620 && segments[1] == 0x004f && segments[2] == 0x8000)
        && !(segments[0] == 0x3fff && segments[1] < 0x1000)
}

pub(super) fn is_pinnable(address: IpAddr) -> bool {
    is_public(address)
        || match address {
            IpAddr::V4(address) => address.is_loopback() || address.is_private(),
            IpAddr::V6(address) => address.is_loopback() || address.is_unique_local(),
        }
}
