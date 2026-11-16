// Copyright 2026 Cisco Systems, Inc. and its affiliates
// SPDX-License-Identifier: Apache-2.0

//! The mDNSResponder backend, for macOS: registers the guide with mDNSResponder,
//! the system's own responder, through its `dns_sd` C API, by way of
//! `async-dnssd`. mDNSResponder owns the records: it announces them, renames
//! the instance on a conflict with another host, and withdraws them when the
//! guide exits, even when it is killed, since it sees the connection close.
//!
//! The SRV record points to the machine's hostname, whose address records
//! mDNSResponder keeps: on an IPv6-only network, for instance, without the
//! CLAT's IPv4 address. So unlike the `mdns-sd` backend, this one leaves out
//! no interface or address of its own.

use std::time::Duration;

use anyhow::Context;
use async_dnssd::Interface;
use async_dnssd::RegisterData;
use async_dnssd::Registration;
use async_dnssd::TxtRecord;
use futures_util::StreamExt;

/// The backend's name, for the log: the responder, not `dns-sd`, which could
/// be taken for DNS-SD, the protocol both backends speak.
pub const NAME: &str = "mDNSResponder";

/// The version `--version` shows, with the backend after it, so that this
/// build can be told apart from the default one.
pub const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (mDNSResponder)");

/// The protocol-independent service type of -01.
const SERVICE_TYPE: &str = "_agents._tcp";

/// The service type with the MCP subtype, as `dns_sd` takes it: after a comma.
const REG_TYPE: &str = "_agents._tcp,_mcp";

/// The one domain to register in: multicast DNS. Without it, mDNSResponder
/// would also register in any default unicast registration domain.
const DOMAIN: &str = "local.";

/// How long to wait for mDNSResponder to resolve the guide's own instance.
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);

/// A registration; dropping it withdraws the announcement.
pub struct Announcement {
    _registration: Registration,
    fullname: String,
    host_name: String,
}

impl Announcement {
    /// Registers the guide, once mDNSResponder has probed the name and
    /// settled on it.
    pub async fn start(instance: &str, port: u16, txt: &[(&str, &str)]) -> anyhow::Result<Self> {
        let txt = txt_rdata(txt)?;
        let data = RegisterData {
            name: Some(instance),
            domain: Some(DOMAIN),
            txt: txt.rdata(),
            ..Default::default()
        };
        let (registration, registered) = async_dnssd::register_extended(REG_TYPE, port, data)
            .context("cannot reach mDNSResponder")?
            .await
            .context("mDNSResponder did not register the guide")?;
        let host_name = own_host_name(&registered.name).await?;
        Ok(Self {
            _registration: registration,
            fullname: full_name(&registered.name, &registered.domain),
            host_name,
        })
    }

    /// The instance's full name, with the name mDNSResponder settled on.
    pub fn fullname(&self) -> &str {
        &self.fullname
    }

    /// The host name the SRV record points to, with its final dot.
    pub fn host_name(&self) -> &str {
        &self.host_name
    }

    /// Withdraws the announcement: mDNSResponder sends the goodbyes.
    pub fn stop(self) {
        drop(self);
    }
}

/// The TXT record in its wire form, with the entries in the order given:
/// [`TxtRecord::set_value`] appends, so `txtvers` stays first.
fn txt_rdata(entries: &[(&str, &str)]) -> anyhow::Result<TxtRecord> {
    let mut txt = TxtRecord::new();
    for (key, value) in entries {
        txt.set_value(key.as_bytes(), value.as_bytes())
            .map_err(|err| anyhow::anyhow!("cannot put `{key}` in the TXT record: {err:?}"))?;
    }
    Ok(txt)
}

/// The full instance name as `mdns-sd` writes it, with the instance label
/// unescaped: `IETF 127 venue guide._agents._tcp.local.`.
fn full_name(instance: &str, domain: &str) -> String {
    format!("{instance}.{SERVICE_TYPE}.{domain}")
}

/// The SRV target mDNSResponder gave the guide: the machine's hostname, such
/// as `LOJAKAB-M-Y2NH.local.`, which it renames too on a conflict. Resolving
/// the guide's own instance finds it.
async fn own_host_name(instance: &str) -> anyhow::Result<String> {
    let mut resolve = async_dnssd::resolve(Interface::Any, instance, SERVICE_TYPE, DOMAIN);
    match tokio::time::timeout(RESOLVE_TIMEOUT, resolve.next()).await {
        Ok(Some(Ok(answer))) => Ok(answer.host_target),
        Ok(Some(Err(err))) => Err(err).context("cannot resolve the guide's own instance"),
        Ok(None) => anyhow::bail!("mDNSResponder ended the resolve without an answer"),
        Err(_) => anyhow::bail!(
            "mDNSResponder did not resolve the guide's own instance within {RESOLVE_TIMEOUT:?}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn keeps_the_txt_entries_in_order() {
        let entries = [("txtvers", "1"), ("proto", "mcp"), ("title", "A guide")];
        let txt = match txt_rdata(&entries) {
            Ok(txt) => txt,
            Err(err) => panic!("{err:#}"),
        };
        let keys: Vec<&[u8]> = txt.iter().map(|(key, _)| key).collect();
        assert_eq!(keys, vec![&b"txtvers"[..], b"proto", b"title"]);
        assert_eq!(txt.rdata(), b"\x09txtvers=1\x09proto=mcp\x0dtitle=A guide");
    }

    #[test]
    fn rejects_keys_that_cannot_be_in_a_txt_record() {
        assert!(txt_rdata(&[("a=b", "1")]).is_err());
    }

    #[test]
    fn writes_full_names_like_mdns_sd() {
        assert_eq!(
            full_name("IETF 127 venue guide", "local."),
            "IETF 127 venue guide._agents._tcp.local."
        );
    }
}
