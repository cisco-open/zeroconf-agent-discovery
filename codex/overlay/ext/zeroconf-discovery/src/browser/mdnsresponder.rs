// Copyright 2026 Cisco Systems, Inc. and its affiliates
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

//! The mDNSResponder backend, for macOS: asks the system's mDNS responder,
//! mDNSResponder, through its `dns_sd` C API, by way of `async-dnssd`.
//! mDNSResponder answers from the system's cache, so servers announced before
//! Codex started appear at once.
//!
//! It reports the same events as the `mdns-sd` backend, by these rules:
//!
//! - It browses `_agents._tcp` in `local.` only. Without a domain,
//!   mDNSResponder would also browse the configured unicast DNS-SD domains,
//!   whose instances the mapping rejects.
//! - mDNSResponder reports an instance once per interface, and removes it per
//!   interface. The instance leaves only when its last interface goes, so
//!   unplugging Ethernet doesn't remove a server still reachable over Wi-Fi.
//! - The full name is the browse result's service name, unescaped, plus
//!   `._agents._tcp.` and the domain, exactly as `mdns-sd` writes it.
//!   `ResolveResult::fullname` is escaped (a space becomes `\032`), which
//!   would change the server names.
//! - Each instance is resolved once, when it is first found. Later changes to
//!   its SRV or TXT record show once it is removed and found again, while
//!   `mdns-sd` follows them in place; servers usually change them by
//!   restarting, which both backends see.
//! - TXT entries keep their wire order, repeated keys included, and the
//!   mapping keeps the first. Entries whose key isn't UTF-8 are dropped, as
//!   `mdns-sd` drops them. A TXT record that doesn't parse skips the instance.
//! - After an error, such as an mDNSResponder restart, every instance is
//!   removed, and browsing starts again a few seconds later.

use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::time::Duration;

use async_dnssd::BrowseData;
use async_dnssd::BrowseResult;
use async_dnssd::BrowsedFlags;
use async_dnssd::Interface;
use async_dnssd::Resolve;
use async_dnssd::TxtRecord;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use super::Event;
use super::THREAD_NAME;
use crate::mapping::ResolvedInstance;

/// The backend's name, for the log.
pub(super) const NAME: &str = "mDNSResponder";

/// The service type, without its domain.
const REG_TYPE: &str = "_agents._tcp";

/// The one domain browsed: multicast DNS.
const DOMAIN: &str = "local.";

/// How long to wait before browsing again after an error.
const RETRY_DELAY: Duration = Duration::from_secs(5);

/// Starts browsing on a thread of its own, with a `tokio` runtime of its own,
/// which keeps discovery off Codex's runtime, and hands each event to `sink`.
pub(super) fn spawn(sink: impl Fn(Event) + Send + 'static) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| err.to_string())?;
    std::thread::Builder::new()
        .name(THREAD_NAME.to_string())
        .spawn(move || {
            tokio::task::LocalSet::new().block_on(&runtime, browse_forever(sink));
        })
        .map(drop)
        .map_err(|err| err.to_string())
}

/// Browses for the life of the process, starting again after every error.
async fn browse_forever(sink: impl Fn(Event)) {
    loop {
        let mut instances = Instances::default();
        let err = browse(&sink, &mut instances).await;
        tracing::warn!(
            "zeroconf discovery: browsing with mDNSResponder stopped: {err}; trying again in {} seconds",
            RETRY_DELAY.as_secs()
        );
        for instance in instances.drain() {
            sink(Event::Removed(instance.fullname.clone()));
        }
        tokio::time::sleep(RETRY_DELAY).await;
    }
}

/// Browses until mDNSResponder reports an error, which it returns.
async fn browse(sink: &impl Fn(Event), instances: &mut Instances) -> io::Error {
    let mut results = async_dnssd::browse_extended(
        REG_TYPE,
        BrowseData {
            domain: Some(DOMAIN),
            ..Default::default()
        },
    );
    let (resolved_tx, mut resolved_rx) = mpsc::unbounded_channel();
    loop {
        tokio::select! {
            result = results.next() => match result {
                Some(Ok(result)) => on_browse_result(result, sink, instances, &resolved_tx),
                Some(Err(err)) => return err,
                None => return io::Error::other("mDNSResponder ended the browse"),
            },
            Some(resolved) = resolved_rx.recv() => on_resolved(resolved, sink, instances),
        }
    }
}

/// Counts the instance in or out on the result's interface: a new instance
/// gets resolved, and one that is gone from its last interface is removed.
fn on_browse_result(
    result: BrowseResult,
    sink: &impl Fn(Event),
    instances: &mut Instances,
    resolved_tx: &mpsc::UnboundedSender<Resolved>,
) {
    let fullname = full_name(&result.service_name, &result.domain);
    if !result.flags.contains(BrowsedFlags::ADD) {
        if let Some(instance) = instances.lost(&fullname, result.interface) {
            sink(Event::Removed(instance.fullname.clone()));
        }
        return;
    }
    let Some(generation) = instances.found(&fullname, result.interface) else {
        return;
    };
    let resolve = result.resolve();
    let key = fullname.to_ascii_lowercase();
    let resolved_tx = resolved_tx.clone();
    let task = tokio::task::spawn_local(async move {
        let outcome = resolve_once(resolve, fullname).await;
        // The receiver is gone only while browsing starts again, and then
        // the instance is gone too.
        let _ = resolved_tx.send(Resolved {
            key,
            generation,
            outcome,
        });
    });
    instances.set_resolving(generation, task.abort_handle());
}

/// Passes a resolution on, unless its instance left, or left and came back,
/// while it was being resolved.
fn on_resolved(resolved: Resolved, sink: &impl Fn(Event), instances: &Instances) {
    if !instances.is_current(&resolved.key, resolved.generation) {
        return;
    }
    match resolved.outcome {
        Ok(instance) => sink(Event::Resolved(instance)),
        Err(err) => tracing::debug!("zeroconf discovery: cannot resolve {}: {err}", resolved.key),
    }
}

/// The first answer of a resolve; the resolve stops when it is dropped.
async fn resolve_once(mut resolve: Resolve, fullname: String) -> Result<ResolvedInstance, String> {
    match resolve.next().await {
        Some(Ok(answer)) => {
            resolved_instance(fullname, answer.host_target, answer.port, &answer.txt)
        }
        Some(Err(err)) => Err(err.to_string()),
        None => Err("mDNSResponder ended the resolve without an answer".to_string()),
    }
}

/// The full instance name as `mdns-sd` writes it, with the service name
/// unescaped: `Lab.Agent` gives `Lab.Agent._agents._tcp.local.`.
fn full_name(service_name: &str, domain: &str) -> String {
    format!("{service_name}.{REG_TYPE}.{domain}")
}

/// A resolve answer in the mapping's terms.
fn resolved_instance(
    fullname: String,
    host: String,
    port: u16,
    txt: &[u8],
) -> Result<ResolvedInstance, String> {
    let record =
        TxtRecord::parse(txt).ok_or_else(|| "the TXT record does not parse".to_string())?;
    let txt = record
        .iter()
        .filter_map(|(key, value)| {
            let key = std::str::from_utf8(key).ok()?;
            Some((key.to_string(), value.map(<[u8]>::to_vec)))
        })
        .collect();
    Ok(ResolvedInstance {
        fullname,
        host,
        port,
        txt,
    })
}

/// A finished resolve, tagged with the instance it was for.
struct Resolved {
    /// The lowercased full name.
    key: String,
    generation: u64,
    outcome: Result<ResolvedInstance, String>,
}

/// The instances mDNSResponder currently reports, by lowercased full name.
#[derive(Default)]
struct Instances {
    by_key: HashMap<String, Instance>,
    last_generation: u64,
}

struct Instance {
    /// The full name as first reported.
    fullname: String,
    /// The interfaces the instance is reported on.
    interfaces: HashSet<Interface>,
    /// Tells this appearance of the instance from earlier ones.
    generation: u64,
    /// The resolve in progress, stopped when the instance goes.
    resolving: Option<AbortHandle>,
}

impl Drop for Instance {
    fn drop(&mut self) {
        if let Some(task) = &self.resolving {
            task.abort();
        }
    }
}

impl Instances {
    /// Counts the instance in on `interface`. Returns the generation of an
    /// instance that is new, which the caller resolves; `None` if it was
    /// already known on another interface.
    fn found(&mut self, fullname: &str, interface: Interface) -> Option<u64> {
        let key = fullname.to_ascii_lowercase();
        if let Some(instance) = self.by_key.get_mut(&key) {
            instance.interfaces.insert(interface);
            return None;
        }
        self.last_generation += 1;
        let generation = self.last_generation;
        self.by_key.insert(
            key,
            Instance {
                fullname: fullname.to_string(),
                interfaces: HashSet::from([interface]),
                generation,
                resolving: None,
            },
        );
        Some(generation)
    }

    /// Counts the instance out on `interface`. Returns the instance once its
    /// last interface is gone.
    fn lost(&mut self, fullname: &str, interface: Interface) -> Option<Instance> {
        let key = fullname.to_ascii_lowercase();
        let instance = self.by_key.get_mut(&key)?;
        instance.interfaces.remove(&interface);
        if instance.interfaces.is_empty() {
            self.by_key.remove(&key)
        } else {
            None
        }
    }

    fn set_resolving(&mut self, generation: u64, task: AbortHandle) {
        if let Some(instance) = self
            .by_key
            .values_mut()
            .find(|instance| instance.generation == generation)
        {
            instance.resolving = Some(task);
        }
    }

    /// Whether the instance is still the appearance that `generation` names.
    fn is_current(&self, key: &str, generation: u64) -> bool {
        self.by_key
            .get(key)
            .is_some_and(|instance| instance.generation == generation)
    }

    /// Takes every instance out.
    fn drain(&mut self) -> Vec<Instance> {
        self.by_key.drain().map(|(_, instance)| instance).collect()
    }
}

#[cfg(test)]
#[path = "mdnsresponder_tests.rs"]
mod tests;
