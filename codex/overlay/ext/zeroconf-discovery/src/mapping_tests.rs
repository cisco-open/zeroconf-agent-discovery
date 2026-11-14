// Copyright 2026 Cisco Systems, Inc. and its affiliates
// SPDX-License-Identifier: Apache-2.0

use pretty_assertions::assert_eq;

use super::*;
use crate::test_support::txt;

fn instance(fullname: &str, host: &str, port: u16, pairs: &[&str]) -> ResolvedInstance {
    ResolvedInstance {
        fullname: fullname.to_string(),
        host: host.to_string(),
        port,
        txt: txt(pairs),
    }
}

/// The MCP example of -01, with the given TXT keys.
fn notes(pairs: &[&str]) -> ResolvedInstance {
    instance("notes._agents._tcp.local.", "room1.local.", 8003, pairs)
}

const NOTES_TXT: &[&str] = &[
    "txtvers=1",
    "proto=mcp",
    "path=/mcp",
    "proto-ver=2025-11-25",
    "desc-ref=urn:air:example.com:rooms:notes",
];

fn map(instance: &ResolvedInstance) -> Result<DiscoveredServer, Skip> {
    map_instance(instance, Policy::default())
}

fn url_of(instance: &ResolvedInstance, policy: Policy) -> Result<String, Skip> {
    map_instance(instance, policy).map(|server| server.url)
}

#[test]
fn maps_the_mcp_example_of_the_draft() {
    assert_eq!(
        map(&notes(NOTES_TXT)),
        Ok(DiscoveredServer {
            name: "lan-notes".to_string(),
            instance: "notes".to_string(),
            url: "https://room1.local:8003/mcp".to_string(),
            title: None,
            desc_ref: Some("urn:air:example.com:rooms:notes".to_string()),
            proto_ver: Some("2025-11-25".to_string()),
        })
    );
}

#[test]
fn builds_the_url_from_srv_and_path() {
    let server = notes(&["txtvers=1", "proto=mcp", "path=/api/mcp"]);
    assert_eq!(
        url_of(&server, Policy::default()),
        Ok("https://room1.local:8003/api/mcp".to_string())
    );
}

#[test]
fn omits_the_default_port() {
    let server = instance(
        "notes._agents._tcp.local.",
        "room1.local.",
        443,
        &["txtvers=1", "proto=mcp", "path=/mcp"],
    );
    assert_eq!(
        url_of(&server, Policy::default()),
        Ok("https://room1.local/mcp".to_string())
    );
}

#[test]
fn txtvers_must_come_first() {
    assert_eq!(
        map(&notes(&["proto=mcp", "txtvers=1", "path=/mcp"])),
        Err(Skip::TxtversNotFirst)
    );
    assert_eq!(
        map(&notes(&["proto=mcp", "path=/mcp"])),
        Err(Skip::TxtversNotFirst)
    );
    assert_eq!(map(&notes(&[])), Err(Skip::TxtversNotFirst));
}

#[test]
fn txtvers_must_be_one() {
    assert_eq!(
        map(&notes(&["txtvers=2", "proto=mcp", "path=/mcp"])),
        Err(Skip::UnsupportedTxtvers("2".to_string()))
    );
    assert_eq!(
        map(&notes(&["txtvers", "proto=mcp", "path=/mcp"])),
        Err(Skip::UnsupportedTxtvers(String::new()))
    );
}

#[test]
fn needs_proto_mcp() {
    assert_eq!(
        map(&notes(&["txtvers=1", "proto=a2a", "path=/mcp"])),
        Err(Skip::NotMcp)
    );
    assert_eq!(map(&notes(&["txtvers=1", "path=/mcp"])), Err(Skip::NotMcp));
    assert_eq!(
        map(&notes(&["txtvers=1", "proto", "path=/mcp"])),
        Err(Skip::NotMcp)
    );
}

#[test]
fn keys_are_case_insensitive() {
    let server = notes(&["TXTVERS=1", "Proto=mcp", "PATH=/mcp"]);
    assert_eq!(
        url_of(&server, Policy::default()),
        Ok("https://room1.local:8003/mcp".to_string())
    );
}

#[test]
fn the_first_occurrence_of_a_key_wins() {
    let server = notes(&[
        "txtvers=1",
        "proto=mcp",
        "path=/first",
        "path=/second",
        "proto=a2a",
    ]);
    assert_eq!(
        url_of(&server, Policy::default()),
        Ok("https://room1.local:8003/first".to_string())
    );
}

#[test]
fn needs_a_plain_path() {
    assert_eq!(
        map(&notes(&["txtvers=1", "proto=mcp"])),
        Err(Skip::MissingPath)
    );
    for path in [
        "path",
        "path=",
        "path=mcp",
        "path=/a b",
        "path=/mcp?x=1",
        "path=/mcp#top",
    ] {
        assert_eq!(
            map(&notes(&["txtvers=1", "proto=mcp", path])),
            Err(Skip::InvalidPath),
            "{path}"
        );
    }
}

#[test]
fn needs_a_valid_host_and_port() {
    let txt = &["txtvers=1", "proto=mcp", "path=/mcp"];
    for host in [
        "",
        ".",
        "room 1.local.",
        "-room.local.",
        "room..local.",
        "room1.local/x.",
    ] {
        assert_eq!(
            map(&instance("notes._agents._tcp.local.", host, 8003, txt)),
            Err(Skip::InvalidHost),
            "{host:?}"
        );
    }
    assert_eq!(
        map(&instance(
            "notes._agents._tcp.local.",
            "room1.local.",
            0,
            txt
        )),
        Err(Skip::InvalidPort)
    );
}

#[test]
fn accepts_a_host_without_the_trailing_dot() {
    let server = instance(
        "notes._agents._tcp.local.",
        "room1.local",
        8003,
        &["txtvers=1", "proto=mcp", "path=/mcp"],
    );
    assert_eq!(
        url_of(&server, Policy::default()),
        Ok("https://room1.local:8003/mcp".to_string())
    );
}

#[test]
fn uses_http_only_for_local_hosts_and_only_when_allowed() {
    let txt = &["txtvers=1", "proto=mcp", "path=/mcp"];
    let local = instance("notes._agents._tcp.local.", "room1.local.", 8003, txt);
    let wide = instance("notes._agents._tcp.local.", "mcp.example.com.", 8003, txt);
    let allow = Policy {
        allow_http_for_local: true,
    };
    assert_eq!(
        url_of(&local, allow),
        Ok("http://room1.local:8003/mcp".to_string())
    );
    assert_eq!(
        url_of(&wide, allow),
        Ok("https://mcp.example.com:8003/mcp".to_string())
    );
    assert_eq!(
        url_of(&local, Policy::default()),
        Ok("https://room1.local:8003/mcp".to_string())
    );
}

#[test]
fn needs_an_agents_instance() {
    let txt = &["txtvers=1", "proto=mcp", "path=/mcp"];
    for fullname in [
        "notes._http._tcp.local.",
        "._agents._tcp.local.",
        "notes",
        "",
    ] {
        assert_eq!(
            map(&instance(fullname, "room1.local.", 8003, txt)),
            Err(Skip::NotAgentsInstance),
            "{fullname:?}"
        );
    }
    let mixed_case = instance("notes._Agents._TCP.local.", "room1.local.", 8003, txt);
    assert_eq!(
        map(&mixed_case).map(|s| s.instance),
        Ok("notes".to_string())
    );
}

#[test]
fn keeps_dots_inside_the_instance_label() {
    let server = instance(
        "Lab.Agent._agents._tcp.local.",
        "room1.local.",
        8003,
        &["txtvers=1", "proto=mcp", "path=/mcp"],
    );
    let mapped = map(&server).map(|s| (s.instance, s.name));
    assert_eq!(
        mapped,
        Ok(("Lab.Agent".to_string(), "lan-lab-agent".to_string()))
    );
}

#[test]
fn drops_display_values_with_control_characters() {
    let mut pairs = vec!["txtvers=1", "proto=mcp", "path=/mcp"];
    pairs.push("title=Notes\u{1b}[31m");
    pairs.push("desc-ref=");
    let mapped = map(&notes(&pairs)).map(|s| (s.title, s.desc_ref));
    assert_eq!(mapped, Ok((None, None)));

    let titled = notes(&["txtvers=1", "proto=mcp", "path=/mcp", "title=Café notes"]);
    assert_eq!(
        map(&titled).map(|s| s.title),
        Ok(Some("Café notes".to_string()))
    );
}

#[test]
fn keeps_proto_ver_only_in_mcp_form() {
    for (value, expected) in [
        ("proto-ver=2025-11-25", Some("2025-11-25")),
        ("proto-ver=latest", None),
        ("proto-ver=2025-1-25", None),
    ] {
        let server = notes(&["txtvers=1", "proto=mcp", "path=/mcp", value]);
        assert_eq!(
            map(&server).map(|s| s.proto_ver),
            Ok(expected.map(str::to_string)),
            "{value}"
        );
    }
}

#[test]
fn sanitizes_instance_names_for_codex() {
    for (instance, expected) in [
        ("notes", "lan-notes"),
        ("Notes (Room 2)", "lan-notes-room-2"),
        ("  --Notes--  ", "lan-notes"),
        ("Café notes", "lan-caf-notes"),
        ("my_server.v2", "lan-my-server-v2"),
    ] {
        assert_eq!(server_name(instance), expected, "{instance:?}");
    }
}

#[test]
fn hashes_names_without_ascii_letters_or_digits() {
    let name = server_name("ノート");
    assert_eq!(name, server_name("ノート"));
    assert_ne!(name, server_name("メモ"));
    assert_eq!(name.len(), NAME_PREFIX.len() + 8);
    assert!(name.starts_with(NAME_PREFIX), "{name}");
}
