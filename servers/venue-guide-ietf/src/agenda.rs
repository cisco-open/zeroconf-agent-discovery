// Copyright 2026 Cisco Systems, Inc. and its affiliates
// SPDX-License-Identifier: Apache-2.0

//! The meeting agenda, as the IETF Datatracker publishes it, and the questions
//! the guide answers about it.
//!
//! Every answer is a fact about the building and its schedule, never a
//! recommendation: the visitor's agent knows what they care about, and does
//! the choosing.

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use chrono::DateTime;
use chrono::Duration;
use chrono::FixedOffset;
use chrono::NaiveDate;
use chrono::NaiveDateTime;
use chrono::NaiveTime;
use chrono::TimeZone;
use chrono::Utc;
use chrono_tz::Tz;
use serde::Deserialize;
use serde::Serialize;

/// The IETF Datatracker, which publishes the agenda and the links in it.
const DATATRACKER: &str = "https://datatracker.ietf.org";

/// Most results a search returns; the rest are only counted.
const SEARCH_LIMIT: usize = 25;

/// Upcoming slots listed for each place.
const PLACE_UPCOMING: usize = 3;

/// The Datatracker's agenda of one meeting, `/api/meeting/<number>/agenda-data`.
/// Only the fields the guide uses are read, and most may be missing.
#[derive(Debug, Deserialize)]
pub struct AgendaData {
    pub meeting: Meeting,
    #[serde(default)]
    pub schedule: Vec<Slot>,
    #[serde(default)]
    pub floors: Vec<Floor>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meeting {
    pub number: String,
    #[serde(default)]
    pub city: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub timezone: String,
}

/// One entry of the schedule: a working group session, a break, a social.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub acronym: String,
    #[serde(default)]
    pub group_name: String,
    #[serde(default)]
    pub group_parent: Option<GroupParent>,
    pub start_date_time: DateTime<FixedOffset>,
    /// Seconds.
    pub duration: f64,
    #[serde(default)]
    pub room: Option<String>,
    #[serde(default)]
    pub location: Option<Location>,
    /// `regular`, `break`, `reg`, `plenary` or `other`.
    #[serde(rename = "type", default)]
    pub kind: String,
    /// `sched`, `canceled`, `resched` and others.
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub agenda: Option<Link>,
    #[serde(default)]
    pub links: Option<Links>,
    #[serde(rename = "isBoF", default)]
    pub is_bof: bool,
    #[serde(default)]
    pub filter_keywords: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct GroupParent {
    #[serde(default)]
    pub acronym: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct Location {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct Link {
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Links {
    #[serde(default)]
    pub chat: Option<String>,
    #[serde(default)]
    pub video_stream: Option<String>,
    /// The slot as an iCalendar file, a path on the Datatracker.
    #[serde(default)]
    pub calendar: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Floor {
    pub name: String,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub rooms: Vec<Room>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Room {
    pub name: String,
    #[serde(default)]
    pub functional_name: String,
}

/// A slot as the guide reports it, in venue time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SlotView {
    pub title: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bof: bool,
    /// Present only when the slot is not on as scheduled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub day: String,
    pub start: String,
    pub end: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agenda: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    /// An iCalendar file of the slot, to add it to a calendar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WhatsOn {
    pub venue_time: String,
    pub on_now: Vec<SlotView>,
    pub starting_soon: Vec<SlotView>,
}

#[derive(Debug, Serialize)]
pub struct Found {
    pub matches: Vec<SlotView>,
    #[serde(skip_serializing_if = "is_zero")]
    pub more: usize,
}

#[derive(Debug, Serialize)]
pub struct Place {
    pub room: String,
    /// Every use the floor plans give the room, such as "Hackathon" and
    /// "Breakout 6" for a room that changes use during the meeting.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub used_for: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floor_plan: Option<String>,
    pub upcoming: Vec<SlotView>,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// The agenda of one meeting, ready for questions.
#[derive(Debug)]
pub struct Agenda {
    data: AgendaData,
    tz: Tz,
    fetched_at: DateTime<Utc>,
    /// Floor name and floor plan by room name, from the floor plans.
    rooms: BTreeMap<String, RoomInfo>,
}

#[derive(Debug)]
struct RoomInfo {
    floor: String,
    floor_plan: Option<String>,
    /// The floor plans list a room once per use, in order.
    used_for: Vec<String>,
}

impl Agenda {
    pub fn from_json(json: &[u8], fetched_at: DateTime<Utc>) -> anyhow::Result<Self> {
        let data: AgendaData =
            serde_json::from_slice(json).context("the agenda is not in the expected format")?;
        let tz: Tz = data
            .meeting
            .timezone
            .parse()
            .map_err(|err| anyhow::anyhow!("unknown time zone in the agenda: {err}"))?;
        // A room that changes use during the meeting is listed once per use,
        // such as Continental 4 for the hackathon and then as a breakout room;
        // keep every use, and the floor of the first listing.
        let mut rooms: BTreeMap<String, RoomInfo> = BTreeMap::new();
        for floor in &data.floors {
            for room in &floor.rooms {
                let info = rooms.entry(room.name.clone()).or_insert_with(|| RoomInfo {
                    floor: floor.name.clone(),
                    floor_plan: floor.image.clone(),
                    used_for: Vec::new(),
                });
                if let Some(used_for) = non_empty(&room.functional_name)
                    && !info.used_for.contains(&used_for)
                {
                    info.used_for.push(used_for);
                }
            }
        }
        Ok(Self {
            data,
            tz,
            fetched_at,
            rooms,
        })
    }

    pub fn meeting(&self) -> &Meeting {
        &self.data.meeting
    }

    pub fn slot_count(&self) -> usize {
        self.data.schedule.len()
    }

    /// What the guide tells a client when it connects.
    pub fn instructions(&self) -> String {
        let meeting = &self.data.meeting;
        let mut days = Vec::new();
        let mut day = meeting.start_date;
        while day <= meeting.end_date {
            days.push(day.format("%a %Y-%m-%d").to_string());
            day = day.succ_opt().unwrap_or(day);
            if days.len() > 14 {
                break;
            }
        }
        format!(
            "An unofficial guide to the IETF {number} meeting venue in {city}, built from the \
             public IETF Datatracker agenda (fetched {fetched} UTC). Meeting days: {days}. \
             All times are venue local time ({tz}). The guide states facts about the schedule \
             and the building; choosing what suits the visitor is up to you.",
            number = meeting.number,
            city = meeting.city,
            fetched = self.fetched_at.format("%Y-%m-%d %H:%M"),
            days = days.join(", "),
            tz = meeting.timezone,
        )
    }

    /// Reads a venue-local time: `YYYY-MM-DD HH:MM`, `YYYY-MM-DDTHH:MM`,
    /// `HH:MM` for today at the venue, or RFC 3339 with an offset.
    pub fn parse_time(&self, text: &str, now: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
        let text = text.trim();
        if let Ok(time) = DateTime::parse_from_rfc3339(text) {
            return Ok(time.with_timezone(&Utc));
        }
        let naive = ["%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M"]
            .iter()
            .find_map(|format| NaiveDateTime::parse_from_str(text, format).ok())
            .or_else(|| {
                NaiveTime::parse_from_str(text, "%H:%M")
                    .ok()
                    .map(|time| now.with_timezone(&self.tz).date_naive().and_time(time))
            })
            .ok_or_else(|| {
                format!("cannot read the time {text:?}; use YYYY-MM-DD HH:MM or HH:MM, venue time")
            })?;
        self.tz
            .from_local_datetime(&naive)
            .earliest()
            .map(|time| time.with_timezone(&Utc))
            .ok_or_else(|| format!("{text:?} does not exist in venue time"))
    }

    /// Reads a venue-local day, `YYYY-MM-DD`.
    pub fn parse_day(text: &str) -> Result<NaiveDate, String> {
        NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d")
            .map_err(|_| format!("cannot read the day {text:?}; use YYYY-MM-DD"))
    }

    /// What is on at `at`, and what starts within `within` after it.
    pub fn whats_on(&self, at: DateTime<Utc>, within: Duration) -> WhatsOn {
        let mut on_now = Vec::new();
        let mut starting_soon = Vec::new();
        for slot in self.sorted_slots() {
            let (start, end) = span(slot);
            if start <= at && at < end {
                on_now.push(self.view(slot));
            } else if at < start && start <= at + within {
                starting_soon.push(self.view(slot));
            }
        }
        WhatsOn {
            venue_time: at
                .with_timezone(&self.tz)
                .format("%a %Y-%m-%d %H:%M")
                .to_string(),
            on_now,
            starting_soon,
        }
    }

    /// Slots matching any of the words in `query`: group acronym or name, slot
    /// name, note, or area. Those matching more words come first, then by time.
    pub fn find_sessions(&self, query: &str, day: Option<NaiveDate>) -> Found {
        let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut scored: Vec<(usize, &Slot)> = self
            .sorted_slots()
            .into_iter()
            .filter(|slot| day.is_none_or(|day| self.local_day(slot) == day))
            .filter_map(|slot| {
                let haystack = haystack(slot);
                let score = terms
                    .iter()
                    .filter(|term| haystack.contains(term.as_str()))
                    .count();
                (score > 0).then_some((score, slot))
            })
            .collect();
        // Stable, so slots with the same score stay in time order.
        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        let more = scored.len().saturating_sub(SEARCH_LIMIT);
        Found {
            matches: scored
                .into_iter()
                .take(SEARCH_LIMIT)
                .map(|(_, slot)| self.view(slot))
                .collect(),
            more,
        }
    }

    /// Rooms whose name or use contains `place`, or where an event whose name
    /// contains it takes place, with what is next in each. For a room found
    /// only by an event, what is next is the next of those events.
    pub fn where_is(&self, place: &str, now: DateTime<Utc>) -> Vec<Place> {
        let needle = place.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        // Each room found, and whether the room itself matched rather than
        // only an event in it: rooms on the floor plans, rooms only the
        // schedule names, then rooms of matching events.
        let mut names: BTreeMap<&str, bool> = BTreeMap::new();
        for (name, info) in &self.rooms {
            if contains(name, &needle) || info.used_for.iter().any(|used| contains(used, &needle)) {
                names.insert(name.as_str(), true);
            }
        }
        for slot in &self.data.schedule {
            let Some(room) = slot.room.as_deref().filter(|room| !room.trim().is_empty()) else {
                continue;
            };
            if contains(room, &needle) {
                names.insert(room, true);
            } else if is_event_named(slot, &needle) {
                names.entry(room).or_insert(false);
            }
        }
        names
            .into_iter()
            .map(|(name, room_matched)| {
                let info = self.rooms.get(name);
                let upcoming = self
                    .sorted_slots()
                    .into_iter()
                    .filter(|slot| slot.room.as_deref() == Some(name) && span(slot).1 > now)
                    .filter(|slot| room_matched || is_event_named(slot, &needle))
                    .take(PLACE_UPCOMING)
                    .map(|slot| self.view(slot))
                    .collect();
                Place {
                    room: name.to_string(),
                    used_for: info.map(|info| info.used_for.clone()).unwrap_or_default(),
                    floor: info.map(|info| info.floor.clone()).or_else(|| {
                        self.data
                            .schedule
                            .iter()
                            .find(|slot| slot.room.as_deref() == Some(name))
                            .and_then(|slot| slot.location.as_ref())
                            .and_then(|location| non_empty(&location.name))
                    }),
                    floor_plan: info.and_then(|info| info.floor_plan.clone()),
                    upcoming,
                }
            })
            .collect()
    }

    /// A session is titled by its working group; anything else by its own
    /// name, since its group is only the team that organizes it (breaks and
    /// registration belong to the Secretariat).
    fn title(&self, slot: &Slot) -> String {
        let group = non_empty(&slot.group_name).map(|group| match non_empty(&slot.acronym) {
            Some(acronym) => format!("{group} ({acronym})"),
            None => group,
        });
        let name = non_empty(&slot.name);
        let title = if slot.kind == "regular" {
            group.or(name)
        } else {
            name.or(group)
        };
        title.unwrap_or_else(|| slot.acronym.clone())
    }

    fn sorted_slots(&self) -> Vec<&Slot> {
        let mut slots: Vec<&Slot> = self.data.schedule.iter().collect();
        slots.sort_by(|a, b| {
            (a.start_date_time, a.room.as_deref()).cmp(&(b.start_date_time, b.room.as_deref()))
        });
        slots
    }

    fn local_day(&self, slot: &Slot) -> NaiveDate {
        slot.start_date_time.with_timezone(&self.tz).date_naive()
    }

    fn view(&self, slot: &Slot) -> SlotView {
        let (start, end) = span(slot);
        let start = start.with_timezone(&self.tz);
        let end = end.with_timezone(&self.tz);
        let title = self.title(slot);
        let floor = slot.room.as_deref().and_then(|room| {
            self.rooms
                .get(room)
                .map(|info| info.floor.clone())
                .or_else(|| slot.location.as_ref().and_then(|l| non_empty(&l.name)))
        });
        let links = slot.links.as_ref();
        SlotView {
            title,
            kind: kind_label(&slot.kind),
            area: slot
                .group_parent
                .as_ref()
                .and_then(|parent| non_empty(&parent.acronym)),
            bof: slot.is_bof,
            status: status_label(&slot.status),
            day: start.format("%a %Y-%m-%d").to_string(),
            start: start.format("%H:%M").to_string(),
            end: end.format("%H:%M").to_string(),
            room: slot.room.as_deref().and_then(non_empty),
            floor,
            note: slot.note.as_deref().and_then(non_empty),
            agenda: slot
                .agenda
                .as_ref()
                .and_then(|link| link.url.as_deref())
                .and_then(non_empty),
            chat: links.and_then(|l| l.chat.as_deref()).and_then(non_empty),
            remote: links
                .and_then(|l| l.video_stream.as_deref())
                .and_then(non_empty),
            calendar: links
                .and_then(|l| l.calendar.as_deref())
                .and_then(non_empty)
                .map(on_datatracker),
        }
    }
}

/// Where the agenda is cached, by meeting number.
pub fn cache_file(cache_dir: &Path, meeting: u32) -> PathBuf {
    cache_dir.join(format!("agenda-{meeting}.json"))
}

/// The Datatracker URL of a meeting's agenda.
pub fn agenda_url(meeting: u32) -> String {
    format!("{DATATRACKER}/api/meeting/{meeting}/agenda-data")
}

/// A link from the agenda as an absolute URL: the agenda gives some links,
/// such as calendar files, as paths on the Datatracker.
fn on_datatracker(link: String) -> String {
    if link.starts_with('/') {
        format!("{DATATRACKER}{link}")
    } else {
        link
    }
}

fn span(slot: &Slot) -> (DateTime<Utc>, DateTime<Utc>) {
    let start = slot.start_date_time.with_timezone(&Utc);
    // Whole seconds are plenty; the agenda uses whole minutes.
    let seconds = slot.duration.max(0.0).round() as i64;
    (start, start + Duration::seconds(seconds))
}

fn haystack(slot: &Slot) -> String {
    let area = slot
        .group_parent
        .as_ref()
        .map(|parent| parent.acronym.as_str())
        .unwrap_or_default();
    let mut text = [
        slot.acronym.as_str(),
        slot.group_name.as_str(),
        slot.name.as_str(),
        slot.note.as_deref().unwrap_or_default(),
        area,
    ]
    .join(" ");
    for keyword in &slot.filter_keywords {
        text.push(' ');
        text.push_str(keyword);
    }
    text.to_lowercase()
}

/// Whether `slot` is an event, anything but a working group session, whose
/// own name contains the needle, such as "Light Breakfast" or "SEC ADs Office
/// Hours". Sessions are found by topic with `find_sessions`, not as places.
fn is_event_named(slot: &Slot, needle_lowercase: &str) -> bool {
    slot.kind != "regular" && contains(&slot.name, needle_lowercase)
}

fn contains(text: &str, needle_lowercase: &str) -> bool {
    text.to_lowercase().contains(needle_lowercase)
}

fn non_empty(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "regular" => "session",
        "plenary" => "plenary",
        "break" => "break",
        "reg" => "registration",
        _ => "other",
    }
}

fn status_label(status: &str) -> Option<String> {
    match status {
        "" | "sched" | "schedw" => None,
        "canceled" => Some("canceled".to_string()),
        "resched" => Some("rescheduled".to_string()),
        other => Some(other.to_string()),
    }
}

#[cfg(test)]
#[path = "agenda_tests.rs"]
mod tests;
