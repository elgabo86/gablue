//! Filtre d'adresses : le partage n'est visible que du réseau local et du
//! tailnet, jamais d'Internet. Pas d'authentification par conception — la
//! seule barrière est « qui peut atteindre le port ».

use std::net::IpAddr;

/// Plage CGNAT de Tailscale (100.64.0.0/10). L'IPv6 du tailnet
/// (`fd7a:115c:a1e0::/48`) est déjà couverte par les adresses locales uniques.
fn is_tailscale_v4(ip: std::net::Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

/// RFC1918 + ULA (`is_private`), lien-local, CGNAT/Tailscale, boucle locale.
/// `allow_nonlocal` existe pour les tests et le dépannage explicite.
pub fn peer_allowed(peer: IpAddr, allow_nonlocal: bool) -> bool {
    if allow_nonlocal {
        return true;
    }
    let peer = peer.to_canonical();
    match peer {
        IpAddr::V4(ip) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local() || is_tailscale_v4(ip)
        }
        IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(s.parse::<Ipv4Addr>().unwrap())
    }

    fn v6(s: &str) -> IpAddr {
        IpAddr::V6(s.parse::<Ipv6Addr>().unwrap())
    }

    #[test]
    fn local_networks_are_allowed() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.9",
            "192.168.1.20",
            "100.64.0.45",
            "100.127.255.254",
        ] {
            assert!(peer_allowed(v4(ip), false), "{ip} devrait passer");
        }
        for ip in ["::1", "fe80::1", "fd7a:115c:a1e0::2d", "fd00::5"] {
            assert!(peer_allowed(v6(ip), false), "{ip} devrait passer");
        }
    }

    #[test]
    fn public_addresses_are_refused() {
        for ip in ["8.8.8.8", "1.1.1.1", "100.63.255.255", "100.128.0.1"] {
            assert!(!peer_allowed(v4(ip), false), "{ip} devrait être refusé");
        }
        for ip in ["2001:4860:4860::8888", "2a01:cb00::1"] {
            assert!(!peer_allowed(v6(ip), false), "{ip} devrait être refusé");
        }
    }

    #[test]
    fn mapped_ipv4_uses_the_v4_rules() {
        assert!(peer_allowed(v6("::ffff:192.168.1.5"), false));
        assert!(!peer_allowed(v6("::ffff:8.8.8.8"), false));
    }

    #[test]
    fn explicit_override_allows_anything() {
        assert!(peer_allowed(v4("8.8.8.8"), true));
    }
}
