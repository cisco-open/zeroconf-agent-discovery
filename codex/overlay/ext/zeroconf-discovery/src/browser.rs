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

//! The browser, which keeps the registry current. A backend does the
//! browsing on a thread of its own and reports each change as an [`Event`];
//! the mapping, the registry and the naming are the same whichever backend
//! reports, so both builds list the same servers under the same names.
//!
//! The backend is chosen at build time (see `build.rs`): `mdns-sd` by
//! default, an mDNS stack of its own, or `mdnsresponder` on macOS, which asks
//! the system's mDNSResponder through its `dns_sd` API.

#[cfg(not(zeroconf_backend = "mdnsresponder"))]
mod mdns_sd;
#[cfg(zeroconf_backend = "mdnsresponder")]
mod mdnsresponder;

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use crate::mapping::DiscoveredServer;
use crate::mapping::Policy;
use crate::mapping::ResolvedInstance;
use crate::mapping::SERVICE_TYPE;
use crate::registry::Change;
use crate::registry::Registry;

#[cfg(not(zeroconf_backend = "mdnsresponder"))]
use self::mdns_sd as backend;
#[cfg(zeroconf_backend = "mdnsresponder")]
use self::mdnsresponder as backend;

/// The name of the thread a backend browses on.
const THREAD_NAME: &str = "zeroconf-browse";

/// A change that a backend reports.
#[derive(Debug)]
pub(crate) enum Event {
    /// An instance was resolved, for the first time or again.
    Resolved(ResolvedInstance),
    /// The instance with this full name left the network.
    Removed(String),
}

/// Browses `_agents._tcp` for the lifetime of the process.
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
        let sink = Arc::clone(&browser);
        match backend::spawn(move |event| sink.apply(event, policy)) {
            Ok(()) => tracing::info!(
                "zeroconf discovery: browsing {SERVICE_TYPE} with {} (plain HTTP for .local: {})",
                backend::NAME,
                policy.allow_http_for_local
            ),
            Err(err) => tracing::warn!(
                "zeroconf discovery: cannot browse {SERVICE_TYPE} with {}: {err}",
                backend::NAME
            ),
        }
        browser
    }

    /// The discovered servers, without those whose name `is_configured`
    /// claims.
    pub(crate) fn servers(&self, is_configured: impl Fn(&str) -> bool) -> Vec<DiscoveredServer> {
        self.registry().servers(is_configured)
    }

    /// Applies one event. Each update takes the lock in a statement of its
    /// own, so the guard is gone before anything is logged.
    fn apply(&self, event: Event, policy: Policy) {
        match event {
            Event::Resolved(instance) => {
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
            Event::Removed(fullname) => {
                let removed = self.registry().removed(&fullname);
                if let Some(server) = removed {
                    tracing::info!("zeroconf discovery: lost {fullname} ({})", server.name);
                }
            }
        }
    }

    fn registry(&self) -> std::sync::MutexGuard<'_, Registry> {
        // Every update leaves the registry consistent, so a panic elsewhere
        // while holding the lock is no reason to stop discovery.
        self.registry.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
