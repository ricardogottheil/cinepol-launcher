use std::net::IpAddr;

/// Where along the path a hop sits, in the terms a player understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HopZone {
    /// The player's own network: Wi-Fi, router (private addresses first).
    Home,
    /// The player's internet provider: carrier-grade NAT and the first
    /// public hops after the home network.
    Provider,
    /// The internet between the provider and the server's network.
    Internet,
    /// The server's network: the last hops before it and the server itself.
    Server,
}

impl HopZone {
    /// Hops that reveal the player's router or provider; hidden from shared
    /// reports unless the player shows them.
    pub fn is_private_to_player(self) -> bool {
        matches!(self, Self::Home | Self::Provider)
    }
}

/// Public hops after the home network that count as the provider's.
pub const PROVIDER_PUBLIC_HOPS: usize = 2;
/// Hops at the end of a reached path that count as the server's network,
/// the server included.
pub const SERVER_HOPS: usize = 2;

/// Addresses that never route on the public internet.
pub fn is_private_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || is_carrier_grade_nat(address)
        }
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
        }
    }
}

/// 100.64.0.0/10, which providers use for carrier-grade NAT.
fn is_carrier_grade_nat(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            a == 100 && (64..128).contains(&b)
        }
        IpAddr::V6(_) => false,
    }
}

/// The zone of every hop, given the address each answered from (`None` when
/// it never answered) and whether the last hop is the server itself.
///
/// Private hops before the first public one are the home network. Carrier-grade
/// NAT hops, private hops after a public or carrier-grade NAT one, and the first
/// [`PROVIDER_PUBLIC_HOPS`] public hops are the provider. When the path reached
/// the server, its last [`SERVER_HOPS`] answering hops outside the home and
/// provider zones are the server's network, and the server itself always is
/// (unless its address is private).
/// Silent hops take the zone of the hop before them.
pub fn classify_hops(addresses: &[Option<IpAddr>], reached: bool) -> Vec<HopZone> {
    let mut zones = Vec::with_capacity(addresses.len());
    let mut past_home = false;
    let mut public_seen = 0usize;
    let mut previous = HopZone::Home;
    for address in addresses {
        let zone = match address {
            None => previous,
            Some(ip) if is_carrier_grade_nat(*ip) => {
                past_home = true;
                HopZone::Provider
            }
            Some(ip) if is_private_address(*ip) => {
                if past_home {
                    HopZone::Provider
                } else {
                    HopZone::Home
                }
            }
            Some(_) => {
                past_home = true;
                public_seen += 1;
                if public_seen <= PROVIDER_PUBLIC_HOPS {
                    HopZone::Provider
                } else {
                    HopZone::Internet
                }
            }
        };
        zones.push(zone);
        previous = zone;
    }
    if reached {
        let answering: Vec<usize> = addresses
            .iter()
            .enumerate()
            .filter(|(_, address)| address.is_some())
            .map(|(index, _)| index)
            .collect();
        let first_server = answering
            .len()
            .checked_sub(SERVER_HOPS)
            .and_then(|start| answering.get(start))
            .or_else(|| answering.first())
            .copied();
        if let Some(first_server) = first_server {
            for zone in zones.iter_mut().skip(first_server) {
                if !zone.is_private_to_player() {
                    *zone = HopZone::Server;
                }
            }
        }
        // The server itself, unless it sits in a private network.
        if let Some(&last) = answering.last()
            && addresses[last].is_some_and(|ip| !is_private_address(ip))
        {
            zones[last] = HopZone::Server;
        }
    }
    zones
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(raw: &str) -> Option<IpAddr> {
        Some(raw.parse().unwrap())
    }

    #[test]
    fn recognizes_private_addresses() {
        for raw in [
            "192.168.1.1",
            "10.0.0.1",
            "172.16.4.1",
            "100.64.0.1",
            "100.127.255.1",
            "169.254.1.1",
            "fd00::1",
            "fe80::1",
        ] {
            assert!(is_private_address(raw.parse().unwrap()), "{raw}");
        }
        for raw in ["200.24.33.104", "100.128.0.1", "1.1.1.1", "2606:4700::1111"] {
            assert!(!is_private_address(raw.parse().unwrap()), "{raw}");
        }
    }

    #[test]
    fn classifies_a_real_path_to_cinepol() {
        // The path measured from Colombia to mc.policarpiers.com.
        let path = [
            ip("192.168.68.1"),
            ip("192.168.1.254"),
            ip("190.248.0.245"),
            ip("200.24.33.104"),
            ip("200.24.33.101"),
            ip("200.24.33.90"),
            ip("206.41.108.129"),
            ip("103.195.101.252"),
        ];
        use HopZone::*;
        assert_eq!(
            classify_hops(&path, true),
            [
                Home, Home, Provider, Provider, Internet, Internet, Server, Server
            ]
        );
    }

    #[test]
    fn carrier_grade_nat_and_later_private_hops_are_the_provider() {
        let path = [
            ip("192.168.0.1"),
            ip("100.70.0.1"),
            ip("10.20.0.1"),
            ip("8.8.8.8"),
        ];
        use HopZone::*;
        assert_eq!(
            classify_hops(&path, false),
            [Home, Provider, Provider, Provider]
        );
    }

    #[test]
    fn silent_hops_take_the_previous_zone_and_an_unreached_path_has_no_server() {
        let path = [
            ip("192.168.0.1"),
            None,
            ip("1.1.1.1"),
            ip("1.0.0.1"),
            ip("9.9.9.9"),
            None,
        ];
        use HopZone::*;
        assert_eq!(
            classify_hops(&path, false),
            [Home, Home, Provider, Provider, Internet, Internet]
        );
    }

    #[test]
    fn a_short_path_never_marks_the_home_network_as_the_server_network() {
        let path = [ip("192.168.0.1"), ip("1.1.1.1")];
        use HopZone::*;
        assert_eq!(classify_hops(&path, true), [Home, Server]);
    }
}
