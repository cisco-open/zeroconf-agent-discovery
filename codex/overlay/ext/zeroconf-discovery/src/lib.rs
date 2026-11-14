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

//! Discovery of MCP servers announced over mDNS/DNS-SD, as specified in
//! draft-jakab-dawn-agent-discovery-mdns.
//!
//! Discovered servers join Codex's MCP catalog disabled: `/mcp` lists them,
//! and Codex connects to none of them on its own. A name the user configured
//! in `config.toml` is never contributed, so the user's own entry for a
//! discovered server, such as one added with `codex mcp add`, takes over.
//!
//! Codex resolves the contributors again for every `/mcp` listing and every
//! model step, so the set updates in a running session. Disabled servers
//! contribute no tools, so a running turn needs no MCP rebuild when the set
//! changes.

use std::sync::Arc;
use std::sync::OnceLock;

use codex_config::McpServerConfig;
use codex_core::config::Config;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::McpServerContribution;
use codex_extension_api::McpServerContributionContext;
use codex_extension_api::McpServerContributor;

mod browser;
pub mod mapping;
pub mod registry;
#[cfg(test)]
mod test_support;

use browser::Browser;
use mapping::Policy;

/// Suffix of every server name this crate contributes: `notes` becomes
/// `notes_local`. It uses `_` because the sanitized instance name never
/// contains one, so no instance can end in the suffix by itself; and a single
/// `_`, because `__` would read as the delimiter of Codex's
/// `mcp__<server>__<tool>` tool names.
pub const NAME_SUFFIX: &str = "_local";

/// Environment variable that turns discovery off when set to `0`.
const ENABLE_VAR: &str = "CODEX_ZEROCONF";

/// Environment variable that, set to `1`, builds `http` URLs for `.local`
/// hosts, for test servers without TLS.
const ALLOW_HTTP_VAR: &str = "CODEX_ZEROCONF_ALLOW_HTTP";

/// The browser, shared by every registry `install` builds.
static BROWSER: OnceLock<Arc<Browser>> = OnceLock::new();

/// Registers the discovery contributor. Codex calls this at startup and again
/// on every MCP reload; the browser starts on the first call and keeps its
/// set across reloads.
pub fn install(builder: &mut ExtensionRegistryBuilder<Config>) {
    if std::env::var(ENABLE_VAR).is_ok_and(|value| value == "0") {
        tracing::info!("zeroconf discovery is off ({ENABLE_VAR}=0)");
        return;
    }
    let browser = BROWSER.get_or_init(|| {
        Browser::start(Policy {
            allow_http_for_local: std::env::var(ALLOW_HTTP_VAR).is_ok_and(|value| value == "1"),
        })
    });
    builder.mcp_server_contributor(Arc::new(ZeroconfDiscovery {
        browser: Arc::clone(browser),
    }));
}

struct ZeroconfDiscovery {
    browser: Arc<Browser>,
}

impl McpServerContributor<Config> for ZeroconfDiscovery {
    fn id(&self) -> &'static str {
        "zeroconf_discovery"
    }

    fn contribute<'a>(
        &'a self,
        context: McpServerContributionContext<'a, Config>,
    ) -> ExtensionFuture<'a, Vec<McpServerContribution>> {
        Box::pin(async move {
            let configured = context.config().mcp_servers.get();
            self.browser
                .servers(|name| configured.contains_key(name))
                .into_iter()
                .filter_map(|server| match disabled_http_server(&server.url) {
                    Ok(config) => Some(McpServerContribution::Set {
                        name: server.name,
                        config: Box::new(config),
                    }),
                    Err(err) => {
                        tracing::warn!(
                            "zeroconf discovery: cannot describe {}: {err}",
                            server.name
                        );
                        None
                    }
                })
                .collect()
        })
    }
}

/// A Streamable HTTP server that Codex lists but does not start. It goes
/// through the parser for `[mcp_servers.<name>]` entries in `config.toml`, so
/// every other field gets the default that such an entry gets.
fn disabled_http_server(url: &str) -> Result<McpServerConfig, serde_json::Error> {
    serde_json::from_value(serde_json::json!({ "url": url, "enabled": false }))
}
