# Venue guide: open tasks

Ideas from the IETF 127 Hackathon that the first version left out. Each entry
says why it matters, which data and code it touches, a possible design, and
how to tell that it is done. They are independent of each other; pick any.

## Before you start

Read [`README.md`](README.md) for building, running and discovery. The code:

| File                  | Contents                                                                                     |
| --------------------- | -------------------------------------------------------------------------------------------- |
| `src/agenda.rs`       | The agenda model, deserialized from the Datatracker, and the queries; pure, no I/O           |
| `src/agenda_tests.rs` | Tests of the queries, on a small invented agenda in the Datatracker's format                 |
| `src/guide.rs`        | The MCP server: one rmcp 3.8 `#[tool]` per query, returning `CallToolResult::structured`     |
| `src/announce.rs`     | The mDNS announcement: type, subtype and TXT record, and the goodbye on shutdown             |
| `src/main.rs`         | The command line, fetching and caching the agenda, the HTTP server, and the shutdown signals |

Conventions the code follows:

- Tools state facts about the schedule and the building; they never
  recommend. The visitor's agent knows the visitor and does the choosing.
- Times are in venue local time, from `meeting.timezone`.
- Tests never use the network or copies of IETF data; extend the invented
  agenda in `agenda_tests.rs` instead.
- Malformed input from the model, such as an unreadable time, is a tool
  result with `isError` and a message that says how to correct it, not a
  protocol error.

Before every push, run `cargo fmt --check`, `cargo clippy --all-targets` and
`cargo test` here. The repository's CI runs super-linter on everything else:
Markdown lines of at most 80 characters outside tables and code, the textlint
terminology rule (write `hostname`, `Git`, and filenames such as
`README.md` in code spans), and no duplicated code at all (JSCPD).

To call the tools by hand, use any MCP client against
`http://localhost:8127/mcp`, such as the MCP Inspector
(`npx @modelcontextprotocol/inspector`). Responses come as server-sent
events, and rmcp keeps the stream open after the answer, so a client that
reads to the end of the response waits forever.

## Tools

### `session_info`: everything about one working group

Asked "when does dnsop meet, and how do I follow it remotely?", the model has
to call `find_sessions("dnsop")`, which also matches every slot that mentions
dnsop in its note. A tool that takes a group acronym and returns all of its
slots, with every link, answers it directly.

- Data: the schedule's slots with that `acronym` (sessions) or
  `groupAcronym`. `links` holds more than `SlotView` reports today: `chat`,
  `chatArchive`, `videoStream`, `audioStream`, `onsiteTool`, `calendar` (an
  `.ics` path relative to the Datatracker) and `recordings`.
- Design: `Agenda::session_info(&self, group: &str) -> Vec<SlotView>`, with
  an exact, case-insensitive acronym match, plus a richer view or an extra
  `links` object, so that `find_sessions` stays compact.
- Done when: a group with two slots returns both, in time order, with all of
  their links, and an unknown acronym returns an empty list.

### `breaks_and_food`, and the hackathon and social events

"When's lunch?" and "is there a reception tonight?" work today only through
`find_sessions` with the right word. A tool for one day's breaks, meals,
registration and social events answers them without guessing names.

- Data: slots of type `break` (Light Breakfast, Beverage Break, Lunch; lunch
  has no room), `reg` (registration), and `other`: receptions, the
  Hackdemo Happy Hour, "IETF Coffee Cart Hours" (in the group of the
  Secretariat, "Application Administrators"), and the hackathon slots
  (group "Hackathon": the hackathon itself, its kickoff and its results
  presentations). Rooms carry a `functionalName` such as "Breaks", "Coffee
  Cart" or "Receptions".
- Design: one tool, for example `events(day, kind)` with the kinds `break`,
  `registration`, `social` and `hackathon`, covers what was discussed as two
  tools (`breaks_and_food` and a hackathon tool). Map slots to kinds by type
  and group, not by name.
- Done when: a fixture day with a break, a lunch without a room, a reception
  and a hackathon slot returns each under its kind.

### A demo action tool

Every tool reads. One tool that does something shows the other half of MCP:
for example `report_issue(room, problem)` for "report a Wi-Fi problem in
this room".

- Design: no real backend. Keep the reports in memory, return a reference
  number, and log them; say in the tool description that it is a
  demonstration and that nothing leaves the guide. Mark it with the MCP tool
  annotations (`readOnlyHint: false`, `destructiveHint: false`), and the read
  tools with `readOnlyHint: true`.
- Done when: a client sees the annotations in `tools/list`, and a report
  shows up in the guide's log.

## Resources and prompts

### Floor plans as MCP resources

`where_is` returns a link to the floor plan image. A client that shows
images, such as Claude, could show the plan itself; Codex's terminal UI shows
none.

- Data: each floor has an `image` URL (a PNG), and each room has pixel
  coordinates (`left`, `right`, `top`, `bottom`), which `Room` doesn't
  deserialize yet.
- Design: enable resources in `ServerCapabilities`, list one resource per
  floor, and serve the image as a base64 blob with its MIME type. Fetch and
  cache the images like the agenda. Highlighting the room on the plan needs
  an image crate; leave it for later.
- Done when: `resources/list` names every floor, and `resources/read`
  returns its PNG.

### A `plan-my-day` prompt

An MCP prompt that asks the agent to combine what the visitor cares about
with `whats_on`, `find_sessions` and `where_is`. Clients offer prompts as
commands or templates.

- Design: rmcp's `#[prompt_router]`, with the day and the topics as
  arguments. Check first which clients show MCP prompts at all.

## Data

### Refresh the agenda while running

The guide fetches the agenda once, at startup, but the Datatracker updates
it during the meeting (`meeting.updated`), for example when a session is
rescheduled.

- Code: `load_agenda` in `src/main.rs`; every `Guide` holds an
  `Arc<Agenda>`.
- Design: keep the agenda behind a shared, swappable handle (an `RwLock` or
  the `arc-swap` crate). A background task fetches it every 30 minutes, and
  on success swaps it in and rewrites the cache; on failure it keeps the
  current one. Clients read the server's instructions, which name the fetch
  time, only when they connect.
- Done when: a fetch that fails keeps answering from the previous agenda,
  and the log says when a new one was loaded.

## Discovery and deployment

### An A2A concierge that shares the guide's `desc-ref`

The draft lets one logical agent offer two protocols, as two instances that
share a `desc-ref`: the guide's tools over MCP, and a goal-oriented
"concierge" over A2A ("plan my Monday"). That is the draft's dual-protocol
case, and nothing implements it yet.

- Design: a descriptor document both instances point to with `desc-ref` (and
  `desc-ref-digest`), an A2A agent, and a second announcement with
  `proto=a2a` and the `_a2a` subtype. This is a project of its own, likely a
  separate crate.

### A public HTTPS instance

Agents that run in the cloud, such as Claude's custom connectors, connect
from their provider's servers, so they can never reach a guide on the
venue's LAN, let alone discover it. They need the guide at a public HTTPS
URL, without an announcement (`--no-announce`). Where it would be hosted is
undecided.

- Needs: TLS in front of the guide, and the public hostname in the allowed
  hosts (see the next task).

### An `--allow-host` option

rmcp rejects any request whose `Host` header isn't on an allowlist, against
DNS rebinding. The guide allows `localhost`, the loopback addresses and the
hostname it announces (`allowed_hosts` in `src/main.rs`), so a client that
connects by IP address, or through a proxy, is turned away.

- Design: a repeatable `--allow-host <name>` added to the list. Never offer a
  way to switch the check off.

### TLS on the LAN

The draft says `https`, and the guide serves plain HTTP, so clients have to
opt in to `http` for `.local` hosts. Serving TLS needs a certificate for a
`.local` name, which no public CA issues; how a client should trust it is an
open question for the draft, so settle that before writing code.

## Engineering

### A CI workflow for this crate

super-linter's Rust checks are off (see the comment in
`.github/workflows/lint.yml`), so nothing in CI builds or tests the guide.

- Design: a workflow that runs `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings` and `cargo test` in
  `servers/venue-guide-ietf/`, on pushes and pull requests that touch it.
  `rust-toolchain.toml` selects the toolchain on GitHub's runners. Follow
  `lint.yml`: actions pinned by commit, `permissions: contents: read`. A
  token that pushes workflow files needs the `workflow` scope.

### Verification gaps

- Discovery between two machines is untested; so far the guide and its
  clients have run on one Mac.
- Linux is untested, both building and announcing next to Avahi.
