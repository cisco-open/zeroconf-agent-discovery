// Copyright 2026 Cisco Systems, Inc. and its affiliates
// SPDX-License-Identifier: Apache-2.0

//! Announcing the guide over mDNS/DNS-SD, as an MCP server in the format of
//! draft-jakab-dawn-agent-discovery-mdns-01.

use std::time::Duration;

use anyhow::Context;
use mdns_sd::ServiceDaemon;
use mdns_sd::ServiceInfo;

/// The protocol-independent service type of -01, with the MCP subtype. Given
/// this type, `mdns-sd` answers for `_agents._tcp` and for its `_mcp` subtype.
const SERVICE_TYPE: &str = "_mcp._sub._agents._tcp.local.";

/// How long to wait for the goodbye to go out on shutdown.
const GOODBYE_WAIT: Duration = Duration::from_secs(1);

/// The TXT record of -01 for an MCP server, in wire order: `txtvers` first.
pub fn txt_record<'a>(
    path: &'a str,
    proto_ver: &'a str,
    title: &'a str,
) -> Vec<(&'static str, &'a str)> {
    vec![
        ("txtvers", "1"),
        ("proto", "mcp"),
        ("path", path),
        ("proto-ver", proto_ver),
        ("title", title),
    ]
}

/// The host name the SRV record points to: `venue-guide-<host>.local.`.
///
/// It is a name of its own, not the machine's: announcing address records for
/// the name the system's responder owns would conflict with it.
pub fn srv_host_name(machine: &str) -> String {
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

/// A registered announcement; dropping it without [`Announcement::stop`]
/// leaves the records to expire with their TTL.
pub struct Announcement {
    daemon: ServiceDaemon,
    fullname: String,
}

impl Announcement {
    pub fn start(
        instance: &str,
        host_name: &str,
        port: u16,
        txt: &[(&str, &str)],
    ) -> anyhow::Result<Self> {
        let daemon = ServiceDaemon::new().context("cannot start the mDNS responder")?;
        let info = ServiceInfo::new(SERVICE_TYPE, instance, host_name, "", port, txt)
            .context("cannot describe the service")?
            .enable_addr_auto();
        let fullname = info.get_fullname().to_string();
        daemon
            .register(info)
            .context("cannot register the service")?;
        Ok(Self { daemon, fullname })
    }

    pub fn fullname(&self) -> &str {
        &self.fullname
    }

    /// Withdraws the announcement, so browsers drop it at once.
    pub fn stop(self) {
        if let Ok(done) = self.daemon.unregister(&self.fullname) {
            let _ = done.recv_timeout(GOODBYE_WAIT);
        }
        let _ = self.daemon.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn txtvers_comes_first() {
        assert_eq!(
            txt_record("/mcp", "2026-07-28", "Guide"),
            vec![
                ("txtvers", "1"),
                ("proto", "mcp"),
                ("path", "/mcp"),
                ("proto-ver", "2026-07-28"),
                ("title", "Guide"),
            ]
        );
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
