//! Filtre d'adresses : le partage n'est visible que du réseau local et du
//! tailnet, jamais d'Internet. Pas d'authentification par conception — la
//! seule barrière est « qui peut atteindre le port ».
//!
//! Le choix de visibilité (`both` / `lan` / `tailscale`) restreint encore cette
//! règle : c'est lui qui décide si un poste du réseau local voit le partage, et
//! s'il est joignable depuis le tailnet.

use crate::config::Visibility;
use std::net::IpAddr;

/// Plage CGNAT de Tailscale (100.64.0.0/10).
fn is_tailscale_v4(ip: std::net::Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

/// Préfixe IPv6 du tailnet (`fd7a:115c:a1e0::/48`).
fn is_tailscale_v6(ip: std::net::Ipv6Addr) -> bool {
    let premier = u32::from(ip.segments()[0]);
    let second = u32::from(ip.segments()[1]);
    premier == 0xfd7a && second == 0x115c && u32::from(ip.segments()[2]) == 0xa1e0
}

/// Adresse du tailnet : c'est ce que voit un poste distant de l'autre côté.
pub fn is_tailnet(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(ip) => is_tailscale_v4(ip),
        IpAddr::V6(ip) => is_tailscale_v6(ip),
    }
}

/// Adresse du réseau local : RFC1918, lien-local, ULA — **hors** plages du
/// tailnet, qui sont traitées à part (une ULA `fd7a:…` est le tailnet, pas le
/// salon).
pub fn is_lan(ip: IpAddr) -> bool {
    let ip = ip.to_canonical();
    if is_tailnet(ip) {
        return false;
    }
    match ip {
        IpAddr::V4(ip) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local()
        }
        IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local(),
    }
}

/// Cette adresse a-t-elle le droit d'atteindre le poste ?
///
/// `allow_nonlocal` reste l'échappatoire de test et de dépannage : elle accepte
/// tout, quelle que soit la visibilité choisie.
pub fn peer_allowed(peer: IpAddr, allow_nonlocal: bool, visibility: Visibility) -> bool {
    if allow_nonlocal {
        return true;
    }
    let peer = peer.to_canonical();
    match visibility {
        Visibility::Both => is_lan(peer) || is_tailnet(peer),
        Visibility::Lan => is_lan(peer),
        // La boucle locale reste toujours autorisée : c'est ainsi que la GUI
        // du poste lui-même l'interroge (`/api/info`, listing, transferts).
        Visibility::Tailscale => peer.is_loopback() || is_tailnet(peer),
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
            assert!(
                peer_allowed(v4(ip), false, Visibility::Both),
                "{ip} devrait passer"
            );
        }
        for ip in ["::1", "fe80::1", "fd7a:115c:a1e0::2d", "fd00::5"] {
            assert!(
                peer_allowed(v6(ip), false, Visibility::Both),
                "{ip} devrait passer"
            );
        }
    }

    #[test]
    fn public_addresses_are_refused() {
        for ip in ["8.8.8.8", "1.1.1.1", "100.63.255.255", "100.128.0.1"] {
            assert!(
                !peer_allowed(v4(ip), false, Visibility::Both),
                "{ip} devrait être refusé"
            );
        }
        for ip in ["2001:4860:4860::8888", "2a01:cb00::1"] {
            assert!(
                !peer_allowed(v6(ip), false, Visibility::Both),
                "{ip} devrait être refusé"
            );
        }
    }

    #[test]
    fn mapped_ipv4_uses_the_v4_rules() {
        assert!(peer_allowed(v6("::ffff:192.168.1.5"), false, Visibility::Both));
        assert!(!peer_allowed(v6("::ffff:8.8.8.8"), false, Visibility::Both));
    }

    #[test]
    fn explicit_override_allows_anything() {
        for visibility in [Visibility::Both, Visibility::Lan, Visibility::Tailscale] {
            assert!(peer_allowed(v4("8.8.8.8"), true, visibility));
        }
    }

    #[test]
    fn lan_only_refuses_the_tailnet() {
        for ip in ["192.168.1.20", "10.0.0.5", "127.0.0.1"] {
            assert!(peer_allowed(v4(ip), false, Visibility::Lan), "{ip}");
        }
        assert!(!peer_allowed(v4("100.64.0.45"), false, Visibility::Lan));
        assert!(!peer_allowed(v6("fd7a:115c:a1e0::2d"), false, Visibility::Lan));
        // Lan only : une ULA du salon passe, celle du tailnet non.
        assert!(peer_allowed(v6("fd00::5"), false, Visibility::Lan));
        assert!(!peer_allowed(v4("8.8.8.8"), false, Visibility::Lan));
    }

    #[test]
    fn tailscale_only_refuses_the_lan_but_keeps_loopback() {
        for ip in ["192.168.1.20", "10.0.0.5", "fe80::1", "fd00::5"] {
            assert!(
                !peer_allowed(parse(ip), false, Visibility::Tailscale),
                "{ip} devrait être refusé"
            );
        }
        for ip in ["100.64.0.45", "fd7a:115c:a1e0::2d", "127.0.0.1", "::1"] {
            assert!(
                peer_allowed(parse(ip), false, Visibility::Tailscale),
                "{ip} devrait passer"
            );
        }
    }

    fn parse(ip: &str) -> IpAddr {
        ip.parse::<IpAddr>().unwrap()
    }

    #[test]
    fn tailnet_is_not_lan() {
        assert!(is_tailnet(v4("100.64.0.1")));
        assert!(!is_lan(v4("100.64.0.1")));
        assert!(is_tailnet(v6("fd7a:115c:a1e0::1")));
        assert!(!is_lan(v6("fd7a:115c:a1e0::1")));
        assert!(is_lan(v4("192.168.0.1")));
    }
}
