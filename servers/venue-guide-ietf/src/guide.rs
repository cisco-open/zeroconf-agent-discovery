// Copyright 2026 Cisco Systems, Inc. and its affiliates
// SPDX-License-Identifier: Apache-2.0

//! The MCP server: the guide's tools over the agenda.

use std::sync::Arc;

use chrono::Duration;
use chrono::Utc;
use rmcp::ErrorData;
use rmcp::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::model::ContentBlock;
use rmcp::model::Implementation;
use rmcp::model::ServerCapabilities;
use rmcp::model::ServerConfig;
use rmcp::schemars;
use rmcp::tool;
use rmcp::tool_handler;
use rmcp::tool_router;
use serde::Deserialize;
use serde::Serialize;

use crate::agenda::Agenda;

/// Default look-ahead of `whats_on`, in minutes.
const DEFAULT_WITHIN_MINUTES: u32 = 90;

/// Longest look-ahead `whats_on` accepts: a day.
const MAX_WITHIN_MINUTES: u32 = 24 * 60;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WhatsOnArgs {
    /// Venue local time to ask about, as "YYYY-MM-DD HH:MM", or "HH:MM" for
    /// today at the venue. Leave it out for the current time.
    #[serde(default)]
    pub at: Option<String>,
    /// How far ahead to list what starts next, in minutes (default 90).
    #[serde(default)]
    pub within_minutes: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FindSessionsArgs {
    /// Words to look for in working group names and acronyms, IETF areas,
    /// session names and notes, such as "dns", "agents" or "sec".
    pub query: String,
    /// Only this venue day, as "YYYY-MM-DD".
    #[serde(default)]
    pub day: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WhereIsArgs {
    /// A room name, such as "Continental 6", or what a room is used for, such
    /// as "coffee", "registration" or "hackathon".
    pub place: String,
}

#[derive(Clone)]
pub struct Guide {
    agenda: Arc<Agenda>,
    tool_router: ToolRouter<Self>,
}

impl Guide {
    pub fn new(agenda: Arc<Agenda>) -> Self {
        Self {
            agenda,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl Guide {
    #[tool(
        description = "What is on at the venue now, or at a given venue time, and what \
                       starts soon after: sessions, breaks, registration and social events, \
                       with room, floor and links. Times are venue local time."
    )]
    fn whats_on(
        &self,
        Parameters(args): Parameters<WhatsOnArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let now = Utc::now();
        let at = match args.at.as_deref() {
            Some(text) => match self.agenda.parse_time(text, now) {
                Ok(at) => at,
                Err(message) => return Ok(invalid(message)),
            },
            None => now,
        };
        let within = args
            .within_minutes
            .unwrap_or(DEFAULT_WITHIN_MINUTES)
            .min(MAX_WITHIN_MINUTES);
        structured(
            &self
                .agenda
                .whats_on(at, Duration::minutes(i64::from(within))),
        )
    }

    #[tool(
        description = "Search the meeting schedule by topic: working group names and \
                       acronyms, IETF areas (such as art, int, ops, rtg, sec, wit, irtf), \
                       session names and notes. Any word may match; slots matching more \
                       words come first. Canceled or rescheduled slots say so. Times are \
                       venue local time."
    )]
    fn find_sessions(
        &self,
        Parameters(args): Parameters<FindSessionsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let day = match args.day.as_deref().map(Agenda::parse_day).transpose() {
            Ok(day) => day,
            Err(message) => return Ok(invalid(message)),
        };
        structured(&self.agenda.find_sessions(&args.query, day))
    }

    #[tool(
        description = "Find a room in the venue by name or by what it is used for (coffee, \
                       registration, hackathon, side meetings, offices): its floor, a link \
                       to the floor plan image, and what is scheduled there next."
    )]
    fn where_is(
        &self,
        Parameters(args): Parameters<WhereIsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        structured(&self.agenda.where_is(&args.place, Utc::now()))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Guide {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(self.agenda.instructions())
    }
}

/// A result as structured content, which rmcp also sends as JSON text.
fn structured<T: Serialize>(value: &T) -> Result<CallToolResult, ErrorData> {
    serde_json::to_value(value)
        .map(CallToolResult::structured)
        .map_err(|err| ErrorData::internal_error(err.to_string(), None))
}

/// A tool error the model can read and correct, such as a malformed time.
fn invalid(message: String) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message)])
}
