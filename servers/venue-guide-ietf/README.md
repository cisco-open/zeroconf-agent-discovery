# Venue guide for IETF meetings

An unofficial guide to the venue of an IETF meeting, as an MCP server that
announces itself on the local network, as specified in
[draft-jakab-dawn-agent-discovery-mdns][draft]. A visitor's agent finds it
without any configuration and asks it what is on, where, and when. The guide
states facts about the schedule and the building; the agent, which knows the
visitor, does the choosing.

It is not affiliated with the IETF. Its data is the public meeting agenda of
the [IETF Datatracker][datatracker].

## Tools

| Tool            | Answers                                                                                 |
| --------------- | --------------------------------------------------------------------------------------- |
| `whats_on`      | What is on now, or at a given venue time, and what starts soon after                    |
| `find_sessions` | Sessions by topic: working group names and acronyms, areas, session names and notes     |
| `where_is`      | A room by name, by use (coffee, hackathon) or by event (breakfast): floor, plan, agenda |

Each result is JSON, with times in venue local time, and links to the agenda,
the chat, the remote stream and a calendar file of each session. Canceled and
rescheduled slots say so.

## Building

You need [rustup](https://rustup.rs). `rust-toolchain.toml` pins the Rust
toolchain, which rustup installs on the first build; that build also
downloads the dependencies from crates.io.

```sh
cd servers/venue-guide-ietf
cargo build --release
```

The result is a single binary, `target/release/venue-guide-ietf`, which can
be copied to another machine of the same platform.

rustup from Homebrew does not put `cargo` on `PATH`, so a shell finds no
`cargo` until its proxies are added:

```sh
export PATH="$(brew --prefix rustup)/bin:$PATH"
```

In fish, run `fish_add_path (brew --prefix rustup)/bin` instead.

## Running

```sh
target/release/venue-guide-ietf --meeting 127
```

`cargo run --release -- --meeting 127` builds and runs in one step. At
startup, the guide fetches the agenda and caches it in
`~/.cache/venue-guide-ietf/`. When the network is down, it uses the cached
copy; `--offline` skips the fetch. It serves MCP over Streamable HTTP on port
8127, at `/mcp`; `--help` lists the options. On Ctrl-C, SIGTERM or SIGHUP, it
withdraws its announcement before it exits, so browsers drop it at once.

## Discovery

The guide announces itself with mDNS as an instance of `_agents._tcp` and of
its `_mcp` subtype, with this TXT record, `txtvers` first:

```text
txtvers=1 proto=mcp path=/mcp proto-ver=2026-07-28 title=Unofficial IETF 127 venue guide
```

The SRV record points to a hostname of its own, `venue-guide-<host>.local`,
not to the hostname of the machine, which belongs to the system's mDNS
responder. To browse for it on macOS:

```sh
dns-sd -B _agents._tcp,_mcp
```

The guide serves plain HTTP, while the draft specifies `https`, so a client
has to allow `http` for `.local` hosts. With the patched Codex CLI in
[`codex/`](../../codex/), set `CODEX_ZEROCONF_ALLOW_HTTP=1`. `/mcp` then lists
the guide as `ietf-127-venue-guide_local`; connect to it with:

```sh
codex mcp add ietf-127-venue-guide_local --url http://venue-guide-<host>.local:8127/mcp
```

Against DNS rebinding, the server answers only requests addressed to
`localhost`, a loopback address, or the hostname it announces. A client on
another machine has to use the announced hostname, not an IP address.

## Testing

```sh
cargo test
cargo clippy --all-targets
cargo fmt --check
```

The tests run on a small invented agenda, without network access.

[datatracker]: https://datatracker.ietf.org/
[draft]: https://datatracker.ietf.org/doc/draft-jakab-dawn-agent-discovery-mdns/
