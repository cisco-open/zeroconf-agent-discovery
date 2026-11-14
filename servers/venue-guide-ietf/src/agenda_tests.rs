// Copyright 2026 Cisco Systems, Inc. and its affiliates
// SPDX-License-Identifier: Apache-2.0

use pretty_assertions::assert_eq;

use super::*;

/// A made-up meeting in the shape of the Datatracker's agenda data, in a
/// venue at UTC-8.
const AGENDA: &str = r#"{
  "meeting": {"number": "999", "city": "Example City", "startDate": "2026-11-14",
              "endDate": "2026-11-16", "timezone": "America/Los_Angeles"},
  "floors": [
    {"name": "Lobby Level", "image": "https://example.org/lobby.png",
     "rooms": [{"name": "East Lounge", "functionalName": "Coffee Cart"},
               {"name": "Plaza A", "functionalName": "Side Meetings (large)"}]},
    {"name": "Ballroom Level", "image": "https://example.org/ballroom.png",
     "rooms": [{"name": "Continental 4", "functionalName": "Hackathon"},
               {"name": "Continental 6", "functionalName": "Breakout 1"}]}
  ],
  "schedule": [
    {"name": "Hackathon", "acronym": "hackathon", "groupName": "", "type": "other",
     "status": "sched", "startDateTime": "2026-11-14T17:30:00+00:00", "duration": 37800,
     "room": "Continental 4", "location": {"name": "Ballroom Level"}},
    {"name": "", "acronym": "dnssd", "groupName": "Extensions for Scalable DNS Service Discovery",
     "groupParent": {"acronym": "int"}, "type": "regular", "status": "sched",
     "startDateTime": "2026-11-16T17:00:00+00:00", "duration": 7200,
     "room": "Continental 6", "location": {"name": "Ballroom Level"},
     "filterKeywords": ["int", "dnssd", "regular"],
     "agenda": {"url": "https://example.org/agenda-dnssd"},
     "links": {"chat": "https://example.org/chat/dnssd", "videoStream": "https://example.org/v/1"}},
    {"name": "", "acronym": "agentbof", "groupName": "Agent Discovery", "isBoF": true,
     "groupParent": {"acronym": "art"}, "type": "regular", "status": "canceled",
     "note": "Discovery of AI agents", "startDateTime": "2026-11-16T17:00:00+00:00",
     "duration": 3600, "room": "Plaza A", "location": {"name": "Lobby Level"},
     "filterKeywords": ["art", "agentbof", "regular", "bof"]},
    {"name": "Beverage Break", "groupName": "Application Administrators",
     "acronym": "secretariat", "type": "break", "status": "sched",
     "startDateTime": "2026-11-16T19:00:00+00:00", "duration": 1800,
     "room": "L & B East Lounges", "location": {"name": "Lobby Level"}},
    {"name": "Lunch", "type": "break", "status": "sched",
     "startDateTime": "2026-11-16T20:30:00+00:00", "duration": 5400, "room": null}
  ]
}"#;

fn agenda() -> Agenda {
    match Agenda::from_json(AGENDA.as_bytes(), utc("2026-11-14T00:00:00Z")) {
        Ok(agenda) => agenda,
        Err(err) => panic!("fixture: {err:#}"),
    }
}

fn utc(text: &str) -> DateTime<Utc> {
    match DateTime::parse_from_rfc3339(text) {
        Ok(time) => time.with_timezone(&Utc),
        Err(err) => panic!("{text}: {err}"),
    }
}

fn titles(slots: &[SlotView]) -> Vec<&str> {
    slots.iter().map(|slot| slot.title.as_str()).collect()
}

#[test]
fn reports_slots_in_venue_time() {
    let found = agenda().find_sessions("dnssd", None);
    assert_eq!(
        found.matches,
        vec![SlotView {
            title: "Extensions for Scalable DNS Service Discovery (dnssd)".to_string(),
            kind: "session",
            area: Some("int".to_string()),
            bof: false,
            status: None,
            day: "Mon 2026-11-16".to_string(),
            start: "09:00".to_string(),
            end: "11:00".to_string(),
            room: Some("Continental 6".to_string()),
            floor: Some("Ballroom Level".to_string()),
            note: None,
            agenda: Some("https://example.org/agenda-dnssd".to_string()),
            chat: Some("https://example.org/chat/dnssd".to_string()),
            remote: Some("https://example.org/v/1".to_string()),
        }]
    );
}

#[test]
fn whats_on_splits_running_and_upcoming() {
    let agenda = agenda();
    // Monday 10:45 in the venue: dnssd runs until 11:00, the break starts at 11:00.
    let on = agenda.whats_on(utc("2026-11-16T18:45:00Z"), Duration::minutes(30));
    assert_eq!(on.venue_time, "Mon 2026-11-16 10:45");
    assert_eq!(
        titles(&on.on_now),
        vec!["Extensions for Scalable DNS Service Discovery (dnssd)"]
    );
    assert_eq!(titles(&on.starting_soon), vec!["Beverage Break"]);
}

#[test]
fn a_slot_ends_when_the_next_one_can_start() {
    let on = agenda().whats_on(utc("2026-11-16T19:00:00Z"), Duration::minutes(0));
    assert_eq!(titles(&on.on_now), vec!["Beverage Break"]);
}

#[test]
fn reads_venue_times() {
    let agenda = agenda();
    let now = utc("2026-11-16T18:00:00Z");
    assert_eq!(
        agenda.parse_time("2026-11-16 09:30", now),
        Ok(utc("2026-11-16T17:30:00Z"))
    );
    assert_eq!(
        agenda.parse_time("2026-11-16T09:30", now),
        Ok(utc("2026-11-16T17:30:00Z"))
    );
    // Today at the venue, not today in UTC.
    assert_eq!(
        agenda.parse_time("13:00", utc("2026-11-17T02:00:00Z")),
        Ok(utc("2026-11-16T21:00:00Z"))
    );
    assert_eq!(
        agenda.parse_time("2026-11-16T09:30:00-08:00", now),
        Ok(utc("2026-11-16T17:30:00Z"))
    );
    assert!(agenda.parse_time("Monday morning", now).is_err());
}

#[test]
fn finds_sessions_by_area_note_or_name_and_flags_changes() {
    let agenda = agenda();
    let found = agenda.find_sessions("agents", None);
    assert_eq!(titles(&found.matches), vec!["Agent Discovery (agentbof)"]);
    assert!(found.matches[0].bof);
    assert_eq!(found.matches[0].status.as_deref(), Some("canceled"));

    let art = agenda.find_sessions("ART", None);
    assert_eq!(titles(&art.matches), vec!["Agent Discovery (agentbof)"]);
}

#[test]
fn more_matching_words_rank_first() {
    let found = agenda().find_sessions("discovery dns", None);
    assert_eq!(
        titles(&found.matches),
        vec![
            "Extensions for Scalable DNS Service Discovery (dnssd)",
            "Agent Discovery (agentbof)",
        ]
    );
}

#[test]
fn filters_by_venue_day() {
    let agenda = agenda();
    let saturday = Agenda::parse_day("2026-11-14").ok();
    let found = agenda.find_sessions("hackathon discovery", saturday);
    assert_eq!(titles(&found.matches), vec!["Hackathon"]);
    assert!(Agenda::parse_day("Saturday").is_err());
}

#[test]
fn finds_places_by_use_with_what_comes_next() {
    let agenda = agenda();
    let coffee = agenda.where_is("coffee", utc("2026-11-16T18:00:00Z"));
    assert_eq!(coffee.len(), 1);
    assert_eq!(coffee[0].room, "East Lounge");
    assert_eq!(coffee[0].used_for.as_deref(), Some("Coffee Cart"));
    assert_eq!(coffee[0].floor.as_deref(), Some("Lobby Level"));
    assert_eq!(
        coffee[0].floor_plan.as_deref(),
        Some("https://example.org/lobby.png")
    );

    let continental = agenda.where_is("continental 6", utc("2026-11-16T18:00:00Z"));
    assert_eq!(
        titles(&continental[0].upcoming),
        vec!["Extensions for Scalable DNS Service Discovery (dnssd)"]
    );
}

#[test]
fn finds_rooms_known_only_from_the_schedule() {
    let places = agenda().where_is("lounges", utc("2026-11-16T18:00:00Z"));
    assert_eq!(places.len(), 1);
    assert_eq!(places[0].room, "L & B East Lounges");
    assert_eq!(places[0].floor.as_deref(), Some("Lobby Level"));
    assert_eq!(places[0].floor_plan, None);
    assert_eq!(titles(&places[0].upcoming), vec!["Beverage Break"]);
}

#[test]
fn instructions_name_the_days_and_the_time_zone() {
    let text = agenda().instructions();
    assert!(text.contains("Sat 2026-11-14, Sun 2026-11-15, Mon 2026-11-16"));
    assert!(text.contains("America/Los_Angeles"));
}
