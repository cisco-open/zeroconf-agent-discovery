// Copyright 2026 Cisco Systems, Inc. and its affiliates
// SPDX-License-Identifier: Apache-2.0

use pretty_assertions::assert_eq;

use super::*;

fn interface(index: u32) -> Interface {
    Interface::from_raw(index)
}

/// TXT RDATA from its strings, each with its length byte.
fn rdata(strings: &[&[u8]]) -> Vec<u8> {
    strings
        .iter()
        .flat_map(|string| {
            std::iter::once(u8::try_from(string.len()).unwrap_or(u8::MAX))
                .chain(string.iter().copied())
        })
        .collect()
}

#[test]
fn the_full_name_keeps_the_service_name_unescaped() {
    assert_eq!(
        full_name("Lab.Agent", "local."),
        "Lab.Agent._agents._tcp.local."
    );
    assert_eq!(
        full_name("Notes (Room 2)", "local."),
        "Notes (Room 2)._agents._tcp.local."
    );
}

#[test]
fn an_instance_leaves_with_its_last_interface() {
    let mut instances = Instances::default();
    let name = "notes._agents._tcp.local.";

    assert!(instances.found(name, interface(4)).is_some());
    assert_eq!(instances.found(name, interface(5)), None);
    assert_eq!(instances.found(name, interface(5)), None);

    assert!(instances.lost(name, interface(4)).is_none());
    let gone = instances.lost(name, interface(5));
    assert_eq!(
        gone.map(|instance| instance.fullname.clone()),
        Some(name.to_string())
    );
    assert!(instances.lost(name, interface(5)).is_none());
}

#[test]
fn instances_match_regardless_of_case() {
    let mut instances = Instances::default();

    assert!(
        instances
            .found("Notes._agents._tcp.local.", interface(4))
            .is_some()
    );
    assert_eq!(
        instances.found("notes._AGENTS._tcp.local.", interface(5)),
        None
    );
    assert!(
        instances
            .lost("NOTES._agents._tcp.local.", interface(4))
            .is_none()
    );
    let gone = instances.lost("notes._agents._tcp.local.", interface(5));
    // The name as first reported.
    assert_eq!(
        gone.map(|instance| instance.fullname.clone()),
        Some("Notes._agents._tcp.local.".to_string())
    );
}

#[test]
fn a_resolution_for_an_earlier_appearance_is_stale() {
    let mut instances = Instances::default();
    let name = "notes._agents._tcp.local.";
    let key = name.to_string();

    let first = instances.found(name, interface(4));
    assert!(first.is_some_and(|generation| instances.is_current(&key, generation)));
    drop(instances.lost(name, interface(4)));
    let second = instances.found(name, interface(4));

    assert_ne!(first, second);
    assert!(first.is_some_and(|generation| !instances.is_current(&key, generation)));
    assert!(second.is_some_and(|generation| instances.is_current(&key, generation)));
}

#[test]
fn txt_entries_keep_wire_order_and_repeated_keys() {
    let txt = rdata(&[
        b"txtvers=1",
        b"proto=mcp",
        b"path=/mcp",
        b"path=/other",
        b"flag",
    ]);

    let instance = resolved_instance(
        "notes._agents._tcp.local.".to_string(),
        "room1.local.".to_string(),
        8003,
        &txt,
    );

    assert_eq!(
        instance.map(|instance| instance.txt),
        Ok(vec![
            ("txtvers".to_string(), Some(b"1".to_vec())),
            ("proto".to_string(), Some(b"mcp".to_vec())),
            ("path".to_string(), Some(b"/mcp".to_vec())),
            ("path".to_string(), Some(b"/other".to_vec())),
            ("flag".to_string(), None),
        ])
    );
}

#[test]
fn txt_entries_whose_key_is_not_utf8_are_dropped() {
    let txt = rdata(&[b"\xff\xfe=1", b"txtvers=1"]);

    let instance = resolved_instance(String::new(), String::new(), 0, &txt);

    assert_eq!(
        instance.map(|instance| instance.txt),
        Ok(vec![("txtvers".to_string(), Some(b"1".to_vec()))])
    );
}

#[test]
fn a_txt_record_that_does_not_parse_skips_the_instance() {
    // The second string claims 9 bytes; 3 follow.
    let txt = [&rdata(&[b"txtvers=1"])[..], &[9, b'a', b'b', b'c']].concat();

    assert!(resolved_instance(String::new(), String::new(), 0, &txt).is_err());
}
