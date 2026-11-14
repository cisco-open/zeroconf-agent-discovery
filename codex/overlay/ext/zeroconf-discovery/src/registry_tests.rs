// Copyright 2026 Cisco Systems, Inc. and its affiliates
// SPDX-License-Identifier: Apache-2.0

use pretty_assertions::assert_eq;

use super::*;
use crate::test_support::txt;

const MCP_TXT: &[&str] = &["txtvers=1", "proto=mcp", "path=/mcp"];

fn instance(fullname: &str, port: u16, pairs: &[&str]) -> ResolvedInstance {
    ResolvedInstance {
        fullname: fullname.to_string(),
        host: "room1.local.".to_string(),
        port,
        txt: txt(pairs),
    }
}

fn mcp(fullname: &str, port: u16) -> ResolvedInstance {
    instance(fullname, port, MCP_TXT)
}

fn names(registry: &Registry) -> Vec<String> {
    registry
        .servers(|_| false)
        .into_iter()
        .map(|server| server.name)
        .collect()
}

#[test]
fn reports_whether_a_resolution_changed_the_set() {
    let mut registry = Registry::default();
    let notes = mcp("notes._agents._tcp.local.", 8003);

    assert_eq!(
        registry.resolved(&notes, Policy::default()),
        Ok(Change::Added)
    );
    assert_eq!(
        registry.resolved(&notes, Policy::default()),
        Ok(Change::Unchanged)
    );
    assert_eq!(
        registry.resolved(&mcp("notes._agents._tcp.local.", 8004), Policy::default()),
        Ok(Change::Updated)
    );
    let servers = registry.servers(|_| false);
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].url, "https://room1.local:8004/mcp");
}

#[test]
fn removals_are_idempotent() {
    let mut registry = Registry::default();
    registry
        .resolved(&mcp("notes._agents._tcp.local.", 8003), Policy::default())
        .ok();

    let removed = registry.removed("notes._agents._tcp.local.");
    assert_eq!(
        removed.map(|server| server.name),
        Some("lan-notes".to_string())
    );
    assert_eq!(registry.removed("notes._agents._tcp.local."), None);
    assert_eq!(names(&registry), Vec::<String>::new());
}

#[test]
fn ignores_the_removal_of_an_unknown_instance() {
    let mut registry = Registry::default();
    registry
        .resolved(&mcp("notes._agents._tcp.local.", 8003), Policy::default())
        .ok();

    assert_eq!(registry.removed("calendar._agents._tcp.local."), None);
    assert_eq!(names(&registry), vec!["lan-notes".to_string()]);
}

#[test]
fn removals_match_regardless_of_case() {
    let mut registry = Registry::default();
    registry
        .resolved(&mcp("Notes._agents._tcp.local.", 8003), Policy::default())
        .ok();

    assert!(registry.removed("notes._AGENTS._tcp.local.").is_some());
    assert_eq!(names(&registry), Vec::<String>::new());
}

#[test]
fn an_instance_that_stops_mapping_leaves_the_set() {
    let mut registry = Registry::default();
    registry
        .resolved(&mcp("notes._agents._tcp.local.", 8003), Policy::default())
        .ok();

    let a2a = instance(
        "notes._agents._tcp.local.",
        8003,
        &["txtvers=1", "proto=a2a", "path=/a2a"],
    );
    assert_eq!(
        registry.resolved(&a2a, Policy::default()),
        Err(Skip::NotMcp)
    );
    assert_eq!(names(&registry), Vec::<String>::new());
}

#[test]
fn leaves_out_names_the_user_configured() {
    let mut registry = Registry::default();
    for fullname in ["notes._agents._tcp.local.", "calendar._agents._tcp.local."] {
        registry
            .resolved(&mcp(fullname, 8003), Policy::default())
            .ok();
    }

    let servers = registry.servers(|name| name == "lan-notes");
    let names: Vec<_> = servers.into_iter().map(|server| server.name).collect();
    assert_eq!(names, vec!["lan-calendar".to_string()]);
}

#[test]
fn the_first_instance_in_order_wins_a_name_collision() {
    let mut registry = Registry::default();
    // Both sanitize to `lan-room-1`; resolve the later one first.
    registry
        .resolved(&mcp("room_1._agents._tcp.local.", 8002), Policy::default())
        .ok();
    registry
        .resolved(&mcp("room 1._agents._tcp.local.", 8001), Policy::default())
        .ok();

    let servers = registry.servers(|_| false);
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].instance, "room 1");

    // Once the winner goes, the other instance takes the name.
    registry.removed("room 1._agents._tcp.local.");
    let servers = registry.servers(|_| false);
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].instance, "room_1");
}

#[test]
fn a_configured_name_does_not_pass_to_a_colliding_instance() {
    let mut registry = Registry::default();
    registry
        .resolved(&mcp("room 1._agents._tcp.local.", 8001), Policy::default())
        .ok();
    registry
        .resolved(&mcp("room_1._agents._tcp.local.", 8002), Policy::default())
        .ok();

    assert_eq!(registry.servers(|name| name == "lan-room-1"), Vec::new());
}
