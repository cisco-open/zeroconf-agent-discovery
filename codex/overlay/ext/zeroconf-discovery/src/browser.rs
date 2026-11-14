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

//! The mDNS browser, which keeps the registry current.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use mdns_sd::ResolvedService;
use mdns_sd::ServiceDaemon;
use mdns_sd::ServiceEvent;

use crate::mapping::DiscoveredServer;
use crate::mapping::Policy;
use crate::mapping::ResolvedInstance;
use crate::mapping::SERVICE_TYPE;
use crate::registry::Change;
use crate::registry::Registry;

/// Browses `_agents._tcp` on every interface, on a thread of its own, for the
/// lifetime of the process.
pub(crate) struct Browser {
    registry: Mutex<Registry>,
}

impl Browser {
    /// Starts browsing. If the browser cannot start, discovery finds nothing,
    /// and Codex works as it does without it.
    pub(crate) fn start(policy: Policy) -> Arc<Self> {
        let browser = Arc::new(Self {
            registry: Mutex::new(Registry::default()),
        });
        match Arc::clone(&browser).spawn(policy) {
            Ok(()) => tracing::info!(
                "zeroconf discovery: browsing {SERVICE_TYPE} (plain HTTP for .local: {})",
                policy.allow_http_for_local
            ),
            Err(err) => tracing::warn!("zeroconf discovery: cannot browse {SERVICE_TYPE}: {err}"),
        }
        browser
    }

    /// The discovered servers, without those whose name `is_configured`
    /// claims.
    pub(crate) fn servers(&self, is_configured: impl Fn(&str) -> bool) -> Vec<DiscoveredServer> {
        self.registry().servers(is_configured)
    }

    fn spawn(self: Arc<Self>, policy: Policy) -> Result<(), String> {
        let daemon = ServiceDaemon::new().map_err(|err| err.to_string())?;
        let events = daemon.browse(SERVICE_TYPE).map_err(|err| err.to_string())?;
        std::thread::Builder::new()
            .name("zeroconf-browse".to_string())
            .spawn(move || {
                // The daemon stops once its last handle is gone, so this
                // thread keeps one.
                let _daemon = daemon;
                while let Ok(event) = events.recv() {
                    self.apply(event, policy);
                }
                tracing::warn!("zeroconf discovery: the mDNS browser stopped");
            })
            .map(drop)
            .map_err(|err| err.to_string())
    }

    /// Applies one event. Each update takes the lock in a statement of its
    /// own, so the guard is gone before anything is logged.
    fn apply(&self, event: ServiceEvent, policy: Policy) {
        match event {
            ServiceEvent::ServiceResolved(service) => {
                let instance = resolved_instance(&service);
                let result = self.registry().resolved(&instance, policy);
                match result {
                    Ok(Change::Added) => {
                        tracing::info!("zeroconf discovery: found {}", instance.fullname);
                    }
                    Ok(Change::Updated) => {
                        tracing::info!("zeroconf discovery: updated {}", instance.fullname);
                    }
                    Ok(Change::Unchanged) => {}
                    Err(skip) => {
                        tracing::debug!(
                            "zeroconf discovery: skipped {}: {skip:?}",
                            instance.fullname
                        );
                    }
                }
            }
            ServiceEvent::ServiceRemoved(_, fullname) => {
                let removed = self.registry().removed(&fullname);
                if let Some(server) = removed {
                    tracing::info!("zeroconf discovery: lost {fullname} ({})", server.name);
                }
            }
            _ => {}
        }
    }

    fn registry(&self) -> std::sync::MutexGuard<'_, Registry> {
        // Every update leaves the registry consistent, so a panic elsewhere
        // while holding the lock is no reason to stop discovery.
        self.registry.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The library's view of an instance, in the mapping's terms. TXT entries keep
/// their wire order; `mdns-sd` has already dropped repeated keys.
fn resolved_instance(service: &ResolvedService) -> ResolvedInstance {
    ResolvedInstance {
        fullname: service.fullname.clone(),
        host: service.host.clone(),
        port: service.port,
        txt: service
            .txt_properties
            .iter()
            .map(|property| {
                (
                    property.key().to_string(),
                    property.val().map(<[u8]>::to_vec),
                )
            })
            .collect(),
    }
}
