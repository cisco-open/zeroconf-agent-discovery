// Copyright 2026 Cisco Systems, Inc. and its affiliates
// SPDX-License-Identifier: Apache-2.0

//! Announcing the guide over mDNS/DNS-SD, as an MCP server in the format of
//! draft-jakab-dawn-agent-discovery-mdns-01, through one of two backends,
//! chosen at build time: `mdns-sd`, a responder of the guide's own, by
//! default; or, with the `mdnsresponder` feature, mDNSResponder, the system's
//! own responder on macOS. Both offer the same [`Announcement`] and name
//! themselves in the log; `--version` names the mDNSResponder one.

#[cfg(all(feature = "mdnsresponder", not(target_os = "macos")))]
compile_error!(
    "the mdnsresponder feature announces through mDNSResponder, so it builds for macOS only"
);

#[cfg(not(all(feature = "mdnsresponder", target_os = "macos")))]
mod mdns_sd;
#[cfg(all(feature = "mdnsresponder", target_os = "macos"))]
mod mdnsresponder;

#[cfg(not(all(feature = "mdnsresponder", target_os = "macos")))]
pub use self::mdns_sd::Announcement;
#[cfg(not(all(feature = "mdnsresponder", target_os = "macos")))]
pub use self::mdns_sd::NAME;
#[cfg(not(all(feature = "mdnsresponder", target_os = "macos")))]
pub use self::mdns_sd::VERSION;
#[cfg(all(feature = "mdnsresponder", target_os = "macos"))]
pub use self::mdnsresponder::Announcement;
#[cfg(all(feature = "mdnsresponder", target_os = "macos"))]
pub use self::mdnsresponder::NAME;
#[cfg(all(feature = "mdnsresponder", target_os = "macos"))]
pub use self::mdnsresponder::VERSION;

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
}
