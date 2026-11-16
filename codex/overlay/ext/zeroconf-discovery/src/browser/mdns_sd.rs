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

//! The `mdns-sd` backend: an mDNS stack of its own, in pure Rust, which
//! behaves the same whatever the operating system does.

use ::mdns_sd::ResolvedService;
use ::mdns_sd::ServiceDaemon;
use ::mdns_sd::ServiceEvent;

use super::Event;
use super::THREAD_NAME;
use crate::mapping::ResolvedInstance;
use crate::mapping::SERVICE_TYPE;

/// The backend's name, for the log.
pub(super) const NAME: &str = "mdns-sd";

/// Starts browsing `_agents._tcp` on every interface, and hands each event to
/// `sink` on a thread of its own.
pub(super) fn spawn(sink: impl Fn(Event) + Send + 'static) -> Result<(), String> {
    let daemon = ServiceDaemon::new().map_err(|err| err.to_string())?;
    let events = daemon.browse(SERVICE_TYPE).map_err(|err| err.to_string())?;
    std::thread::Builder::new()
        .name(THREAD_NAME.to_string())
        .spawn(move || {
            // The daemon stops once its last handle is gone, so this thread
            // keeps one.
            let _daemon = daemon;
            while let Ok(event) = events.recv() {
                if let Some(event) = translate(event) {
                    sink(event);
                }
            }
            tracing::warn!("zeroconf discovery: the mDNS browser stopped");
        })
        .map(drop)
        .map_err(|err| err.to_string())
}

/// The events the browser acts on; `mdns-sd` reports others, such as a type
/// found or a search started, which it ignores.
fn translate(event: ServiceEvent) -> Option<Event> {
    match event {
        ServiceEvent::ServiceResolved(service) => {
            Some(Event::Resolved(resolved_instance(&service)))
        }
        ServiceEvent::ServiceRemoved(_, fullname) => Some(Event::Removed(fullname)),
        _ => None,
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
