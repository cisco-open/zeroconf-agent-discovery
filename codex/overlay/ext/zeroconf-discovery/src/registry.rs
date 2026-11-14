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

//! The set of MCP servers currently advertised on the network.
//!
//! mDNS events are hints, not a log: a removal can repeat, or arrive for an
//! instance that was never resolved, and a resolution can repeat unchanged.
//! The registry therefore keeps a set keyed by instance, and applying an event
//! twice has the same effect as applying it once.

use std::collections::BTreeMap;
use std::collections::HashSet;

use crate::mapping::DiscoveredServer;
use crate::mapping::Policy;
use crate::mapping::ResolvedInstance;
use crate::mapping::Skip;
use crate::mapping::map_instance;

/// What a resolution did to the set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// The server joined the set.
    Added,
    /// The server was in the set with other values, for example another URL.
    Updated,
    /// The server was already in the set with the same values.
    Unchanged,
}

/// The advertised MCP servers, by instance.
#[derive(Debug, Default)]
pub struct Registry {
    /// Keyed by the lowercased full instance name: DNS names compare without
    /// regard to ASCII case, so a removal must match however it is spelled.
    servers: BTreeMap<String, DiscoveredServer>,
}

impl Registry {
    /// Applies a resolved instance. An instance that no longer maps, for
    /// example after its TXT record changed, leaves the set.
    pub fn resolved(
        &mut self,
        instance: &ResolvedInstance,
        policy: Policy,
    ) -> Result<Change, Skip> {
        let key = instance.fullname.to_ascii_lowercase();
        let server = match map_instance(instance, policy) {
            Ok(server) => server,
            Err(skip) => {
                self.servers.remove(&key);
                return Err(skip);
            }
        };
        let change = match self.servers.get(&key) {
            None => Change::Added,
            Some(known) if *known == server => Change::Unchanged,
            Some(_) => Change::Updated,
        };
        self.servers.insert(key, server);
        Ok(change)
    }

    /// Applies a removal, and returns the server it removed, if any.
    pub fn removed(&mut self, fullname: &str) -> Option<DiscoveredServer> {
        self.servers.remove(&fullname.to_ascii_lowercase())
    }

    /// The servers to contribute, in instance order, without those whose name
    /// `is_configured` claims. Two instances can map to the same server name;
    /// the first in instance order wins, so the choice stays the same for as
    /// long as both are advertised.
    pub fn servers(&self, is_configured: impl Fn(&str) -> bool) -> Vec<DiscoveredServer> {
        let mut seen = HashSet::new();
        self.servers
            .values()
            .filter(|server| !is_configured(&server.name))
            .filter(|server| seen.insert(server.name.as_str()))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
