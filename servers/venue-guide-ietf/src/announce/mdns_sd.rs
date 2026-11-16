// Copyright 2026 Cisco Systems, Inc. and its affiliates
// SPDX-License-Identifier: Apache-2.0

//! The `mdns-sd` backend, the default: a responder of the guide's own, next
//! to the system's. It owns all of the guide's records, the address records
//! of a hostname of its own included, and withdraws them when the guide
//! stops; if the guide is killed, they stay in caches until their TTL ends.

use std::net::IpAddr;
use std::time::Duration;

use anyhow::Context;
use mdns_sd::IfKind;
use mdns_sd::IfPredicate;
use mdns_sd::ServiceDaemon;
use mdns_sd::ServiceInfo;

/// The backend's name, for the log.
pub const NAME: &str = "mdns-sd";

/// The version `--version` shows; as in the Codex build, the default backend
/// goes unnamed.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The protocol-independent service type of -01, with the MCP subtype. Given
/// this type, `mdns-sd` answers for `_agents._tcp` and for its `_mcp` subtype.
const SERVICE_TYPE: &str = "_mcp._sub._agents._tcp.local.";

/// How long to wait for the daemon to confirm the first goodbye.
const GOODBYE_WAIT: Duration = Duration::from_secs(1);

/// `mdns-sd` repeats each goodbye 120 ms after the first, for peers that
/// missed it; shutting the daemon down sooner would cancel the repeat.
const GOODBYE_REPEAT_WAIT: Duration = Duration::from_millis(250);

/// The host name the SRV record points to: `venue-guide-<host>.local.`.
///
/// It is a name of its own, not the machine's: announcing address records for
/// the name the system's responder owns would conflict with it.
fn srv_host_name(machine: &str) -> String {
    let label: String = machine
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let label = label.trim_matches('-');
    if label.is_empty() {
        "venue-guide.local.".to_string()
    } else {
        format!("venue-guide-{label}.local.")
    }
}

/// The loopback interfaces, to leave out of the announcement: no other host
/// can reach them. On loopback, `mdns-sd` announces the same names with other
/// addresses, which it can take for another host's and rename the guide; and
/// since every link-local address is in `fe80::/64`, loopback's would be
/// announced on the LAN too.
///
/// By name, as macOS also gives `lo0` the link-local `fe80::1`, which neither
/// [`IfKind::LoopbackV4`] nor [`IfKind::LoopbackV6`] matches: they match the
/// loopback addresses alone, which stay as a fallback.
fn loopback_interfaces() -> Vec<IfKind> {
    let mut names: Vec<String> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|intf| intf.is_loopback())
        .map(|intf| intf.name)
        .collect();
    names.sort();
    names.dedup();
    let mut kinds = vec![IfKind::LoopbackV4, IfKind::LoopbackV6];
    kinds.extend(names.into_iter().map(IfKind::Name));
    kinds
}

/// Whether `ip` is in the IPv4 Service Continuity Prefix, `192.0.0.0/29`
/// (RFC 7335), where a CLAT (RFC 6877) takes its host's IPv4 address on an
/// IPv6-only network: `192.0.0.2` on macOS and iOS, `192.0.0.4` on Android.
/// The CLAT translates it before a packet leaves the host, and every other
/// host may have the same address, so a peer that connects to it reaches
/// itself.
fn is_service_continuity(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            let [a, b, c, d] = ipv4.octets();
            [a, b, c] == [192, 0, 0] && d < 8
        }
        IpAddr::V6(_) => false,
    }
}

/// A registered announcement; dropping it without [`Announcement::stop`]
/// leaves the records to expire with their TTL.
pub struct Announcement {
    daemon: ServiceDaemon,
    fullname: String,
    host_name: String,
}

impl Announcement {
    /// Starts answering for the guide. `async` only to match the
    /// mDNSResponder backend: the daemon probes and announces on its own
    /// thread.
    pub async fn start(instance: &str, port: u16, txt: &[(&str, &str)]) -> anyhow::Result<Self> {
        let host_name = srv_host_name(&gethostname::gethostname().to_string_lossy());
        let daemon = ServiceDaemon::new().context("cannot start the mDNS responder")?;
        // Before registering, so that none of these addresses is announced.
        daemon
            .disable_interface(loopback_interfaces())
            .context("cannot leave out the loopback interface")?;
        daemon
            .disable_interface(IfKind::Predicate(IfPredicate::new(|intf| {
                is_service_continuity(intf.ip())
            })))
            .context("cannot leave out the CLAT address")?;
        let info = ServiceInfo::new(SERVICE_TYPE, instance, &host_name, "", port, txt)
            .context("cannot describe the service")?
            .enable_addr_auto();
        let fullname = info.get_fullname().to_string();
        daemon
            .register(info)
            .context("cannot register the service")?;
        Ok(Self {
            daemon,
            fullname,
            host_name,
        })
    }

    /// The instance's full name, as registered; `mdns-sd` may rename it on a
    /// conflict without telling.
    pub fn fullname(&self) -> &str {
        &self.fullname
    }

    /// The host name the SRV record points to, with its final dot.
    pub fn host_name(&self) -> &str {
        &self.host_name
    }

    /// Withdraws the announcement, so browsers drop it at once.
    pub fn stop(self) {
        if let Ok(done) = self.daemon.unregister(&self.fullname) {
            let _ = done.recv_timeout(GOODBYE_WAIT);
            std::thread::sleep(GOODBYE_REPEAT_WAIT);
        }
        let _ = self.daemon.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// Every system has a loopback interface, and it is left out by name.
    #[test]
    fn leaves_out_the_loopback_interface_by_name() {
        let kinds = loopback_interfaces();
        assert!(
            kinds.iter().any(|kind| matches!(kind, IfKind::Name(_))),
            "{kinds:?}"
        );
    }

    fn ip(text: &str) -> IpAddr {
        match text.parse() {
            Ok(ip) => ip,
            Err(err) => panic!("{text}: {err}"),
        }
    }

    /// The whole `/29`, and none of the rest of `192.0.0.0/24`, such as the
    /// IPv4 dummy address (RFC 7600) or the NAT64 discovery addresses (RFC
    /// 7050).
    #[test]
    fn recognizes_clat_addresses() {
        for clat in ["192.0.0.0", "192.0.0.2", "192.0.0.4", "192.0.0.7"] {
            assert!(is_service_continuity(ip(clat)), "{clat}");
        }
        for other in [
            "192.0.0.8",
            "192.0.0.170",
            "192.0.2.2",
            "10.0.0.2",
            "2001:db8::2",
        ] {
            assert!(!is_service_continuity(ip(other)), "{other}");
        }
    }

    #[test]
    fn the_srv_host_is_derived_from_the_machine_name() {
        assert_eq!(
            srv_host_name("LOJAKAB-M-1234.example.com"),
            "venue-guide-lojakab-m-1234.local."
        );
        assert_eq!(srv_host_name("Lori's Mac"), "venue-guide-lori-s-mac.local.");
        assert_eq!(srv_host_name(""), "venue-guide.local.");
    }
}
