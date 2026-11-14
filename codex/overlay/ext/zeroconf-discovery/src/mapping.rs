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

//! From a resolved DNS-SD service instance to an MCP server, following the
//! MCP instantiation in draft-jakab-dawn-agent-discovery-mdns-01.
//!
//! Everything in an advertisement is an unauthenticated hint (-01, Security
//! Considerations), so the mapping validates what it uses to build the URL
//! and drops display values that could carry terminal control characters.

use url::Url;

use crate::NAME_SUFFIX;

/// The protocol-independent service type of -01, in the mDNS domain.
pub const SERVICE_TYPE: &str = "_agents._tcp.local.";

/// A resolved service instance, independent of the mDNS library.
#[derive(Clone, Debug)]
pub struct ResolvedInstance {
    /// Full service instance name, for example `notes._agents._tcp.local.`.
    pub fullname: String,
    /// SRV target host name, for example `room1.local.`.
    pub host: String,
    /// SRV port.
    pub port: u16,
    /// TXT key/value pairs in wire order; `None` for a key without `=`.
    pub txt: Vec<(String, Option<Vec<u8>>)>,
}

/// Local policy for the URLs built from advertisements.
#[derive(Clone, Copy, Debug, Default)]
pub struct Policy {
    /// Use `http` instead of `https` for hosts in `.local`. -01 always uses
    /// `https`; this exists for test servers without TLS.
    pub allow_http_for_local: bool,
}

/// An MCP server built from an advertisement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredServer {
    /// Codex MCP server name: the sanitized instance name plus the suffix.
    pub name: String,
    /// The instance name as advertised.
    pub instance: String,
    /// The reconstructed MCP endpoint URL.
    pub url: String,
    /// Display name chosen by the advertiser; unverified.
    pub title: Option<String>,
    /// Reference to an external descriptor of the agent.
    pub desc_ref: Option<String>,
    /// An MCP protocol version the server supports; a hint only.
    pub proto_ver: Option<String>,
}

/// Why an advertisement does not yield an MCP server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Skip {
    /// The instance is not of type `_agents._tcp.local.`.
    NotAgentsInstance,
    /// The first TXT key is not `txtvers`.
    TxtversNotFirst,
    /// A TXT record format version other than `1`.
    UnsupportedTxtvers(String),
    /// `proto` is absent or names another agent protocol.
    NotMcp,
    /// No `path` key; MCP defines no default path.
    MissingPath,
    /// A `path` that is empty or not a plain URL path.
    InvalidPath,
    /// An SRV target that is not a valid host name.
    InvalidHost,
    /// SRV port 0.
    InvalidPort,
}

/// Maps a resolved instance to an MCP server, or says why it doesn't map.
pub fn map_instance(instance: &ResolvedInstance, policy: Policy) -> Result<DiscoveredServer, Skip> {
    let label = instance_label(&instance.fullname).ok_or(Skip::NotAgentsInstance)?;

    match instance.txt.first() {
        Some((key, value)) if key.eq_ignore_ascii_case("txtvers") => {
            let version = value
                .as_deref()
                .map(String::from_utf8_lossy)
                .unwrap_or_default();
            if version != "1" {
                return Err(Skip::UnsupportedTxtvers(version.into_owned()));
            }
        }
        _ => return Err(Skip::TxtversNotFirst),
    }

    if txt_value(&instance.txt, "proto") != Some(Some(b"mcp".as_slice())) {
        return Err(Skip::NotMcp);
    }

    let path = match txt_value(&instance.txt, "path") {
        None => return Err(Skip::MissingPath),
        Some(value) => value
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .filter(|path| is_plain_path(path))
            .ok_or(Skip::InvalidPath)?,
    };

    let host = instance.host.strip_suffix('.').unwrap_or(&instance.host);
    if !is_host_name(host) {
        return Err(Skip::InvalidHost);
    }
    if instance.port == 0 {
        return Err(Skip::InvalidPort);
    }

    let is_local = host
        .rsplit('.')
        .next()
        .is_some_and(|tld| tld.eq_ignore_ascii_case("local"));
    let scheme = if policy.allow_http_for_local && is_local {
        "http"
    } else {
        "https"
    };
    let port = instance.port;
    let url = Url::parse(&format!("{scheme}://{host}:{port}{path}"))
        .map_err(|_| Skip::InvalidPath)?
        .to_string();

    Ok(DiscoveredServer {
        name: server_name(label),
        instance: label.to_string(),
        url,
        title: display_value(&instance.txt, "title"),
        desc_ref: display_value(&instance.txt, "desc-ref"),
        proto_ver: display_value(&instance.txt, "proto-ver").filter(|v| is_mcp_version(v)),
    })
}

/// Codex server name for an instance: the instance name lowercased, with
/// every run of characters other than ASCII letters and digits turned into one
/// `-`, then the suffix. An instance name with no ASCII letters or digits gets
/// a stable hash instead.
pub fn server_name(instance: &str) -> String {
    let mut sanitized = String::with_capacity(instance.len());
    let mut separator = false;
    for c in instance.chars() {
        if c.is_ascii_alphanumeric() {
            if separator && !sanitized.is_empty() {
                sanitized.push('-');
            }
            separator = false;
            sanitized.push(c.to_ascii_lowercase());
        } else {
            separator = true;
        }
    }
    if sanitized.is_empty() {
        let hash = fnv1a(instance.as_bytes());
        format!("{hash:08x}{NAME_SUFFIX}")
    } else {
        format!("{sanitized}{NAME_SUFFIX}")
    }
}

/// The instance label of a full service instance name. Labels can contain
/// dots, so the known suffix is stripped instead of splitting at the first dot.
fn instance_label(fullname: &str) -> Option<&str> {
    let split = fullname.len().checked_sub(SERVICE_TYPE.len() + 1)?;
    let (label, suffix) = (fullname.get(..split)?, fullname.get(split..)?);
    let is_agents = suffix
        .strip_prefix('.')
        .is_some_and(|rest| rest.eq_ignore_ascii_case(SERVICE_TYPE));
    (is_agents && !label.is_empty()).then_some(label)
}

/// The value of the first occurrence of `key` (RFC 6763, Section 6.4: keys
/// are case-insensitive, and only the first occurrence counts). The outer
/// `None` means the key is absent, the inner one that it has no `=`.
fn txt_value<'a>(txt: &'a [(String, Option<Vec<u8>>)], key: &str) -> Option<Option<&'a [u8]>> {
    txt.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.as_deref())
}

/// A non-empty UTF-8 value without control characters, or `None`.
fn display_value(txt: &[(String, Option<Vec<u8>>)], key: &str) -> Option<String> {
    let value = std::str::from_utf8(txt_value(txt, key)??).ok()?;
    (!value.is_empty() && !value.chars().any(char::is_control)).then(|| value.to_string())
}

/// A URL path of visible ASCII starting with `/`, without query or fragment.
fn is_plain_path(path: &str) -> bool {
    path.starts_with('/')
        && path
            .bytes()
            .all(|b| b.is_ascii_graphic() && b != b'?' && b != b'#')
}

/// Dot-separated labels of ASCII letters, digits, `-` and `_`, each 1 to 63
/// octets and not starting or ending with `-`.
fn is_host_name(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

/// The MCP `YYYY-MM-DD` version form.
fn is_mcp_version(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            _ => b.is_ascii_digit(),
        })
}

/// 32-bit FNV-1a, for stable names; not for security.
fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |hash, b| {
        (hash ^ u32::from(*b)).wrapping_mul(0x0100_0193)
    })
}

#[cfg(test)]
#[path = "mapping_tests.rs"]
mod tests;
