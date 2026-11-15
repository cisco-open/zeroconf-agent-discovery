# Zeroconf Agent Discovery

[![Release](https://img.shields.io/github/v/release/cisco-open/zeroconf-agent-discovery?display_name=tag)](CHANGELOG.md)
[![Lint](https://github.com/cisco-open/zeroconf-agent-discovery/actions/workflows/lint.yml/badge.svg?branch=main)](https://github.com/marketplace/actions/super-linter)
[![Contributor-Covenant](https://img.shields.io/badge/Contributor%20Covenant-2.1-fbab2c.svg)](CODE_OF_CONDUCT.md)
[![Maintainer](https://img.shields.io/badge/Maintainer-Cisco-00bceb.svg)](https://opensource.cisco.com)

Discover AI agents on the local network with plain mDNS/DNS-SD, as specified in
[Zero-Configuration Agent Discovery][draft].

## About The Project

Agent protocols such as the Model Context Protocol (MCP) and Agent2Agent (A2A)
find agents through documents fetched over HTTPS from a well-known URI. That
assumes the client already knows a hostname or a catalog, and on a local
network there is often neither.

[draft-jakab-dawn-agent-discovery-mdns][draft] advertises agents with DNS-Based
Service Discovery (DNS-SD, [RFC 6763][rfc6763]) over Multicast DNS (mDNS,
[RFC 6762][rfc6762]). It uses one service type, `_agents._tcp`, and one service
instance per agent protocol, named by the `proto` TXT key and optionally by a
subtype (`_mcp._sub`, `_a2a._sub`). A few more TXT keys tell a client, before it
connects, how to reach the endpoint, which logical agent it belongs to
(`desc-ref`), and whether its cached description is still current
(`desc-ref-digest`).

This repository holds implementations of the draft and the tooling used to test
them against each other. Discovery records are not authenticated, so the clients
here list what they find and connect only when the user asks them to.

### Components

| Component                                                                                                                                                       | Status                           |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| Command-line tools to announce and browse `_agents._tcp` instances, in Rust on top of [`mdns-sd`][mdns-sd]                                                      | Planned                          |
| [An unofficial IETF meeting venue guide](servers/venue-guide-ietf/): an MCP server with tools over the live meeting agenda, which announces itself              | Works                            |
| A2A agents that announce themselves, including one that offers both protocols as two instances sharing one `desc-ref`                                           | Planned                          |
| MCP server discovery for [Codex CLI][codex]: a patch series against a pinned upstream commit, with a script that applies it and builds the CLI                  | [In progress](#codex-cli-status) |
| Interoperability tests across Apple mDNSResponder, Avahi, [python-zeroconf][python-zeroconf] and the Rust [`mdns-sd`][mdns-sd] crate, subtype browsing included | Planned                          |

The first target is the [IETF 127 Hackathon][hackathon] in November 2026.

The project publishes source code only. It distributes no binaries: the patched
Codex CLI is built locally, from upstream source.

### Codex CLI status

The goal for the hackathon: MCP servers announced on the local network show up
in the `/mcp` list of Codex CLI, under a heading of their own and not
connected, so that Codex connects to none of them until the user asks it to.

| Capability                                                                                                                                                                     | Status                                       |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------- |
| Build the patched CLI from the pinned upstream release, with `codex/build.sh`                                                                                                  | Works on macOS; untested on Linux            |
| Run the discovery code inside the Codex app-server                                                                                                                             | Works                                        |
| Turn an MCP advertisement in the format of [draft -01][draft] into a Codex server entry: TXT checks, the URL from the SRV target, port and `path`, the name `<instance>_local` | Works, with unit tests                       |
| Browse `_agents._tcp` on the local network                                                                                                                                     | Works on one host; across hosts untested     |
| List the discovered servers in `/mcp`, apart and not connected                                                                                                                 | Works; they come and go in a running session |
| Connect to a discovered server                                                                                                                                                 | Works, with `codex mcp add`                  |

Build and run instructions are in [codex/README.md](codex/README.md).

## Getting Started

### Prerequisites

- macOS or Linux. mDNS is link-local, so announcers and browsers must share a
  link, on a network that forwards multicast.
- [rustup](https://rustup.rs), to build the venue guide and the patched Codex
  CLI. Each pins its Rust toolchain, which rustup installs when it is missing.

### Installation

1. Clone the repository

   ```sh
   git clone https://github.com/cisco-open/zeroconf-agent-discovery.git
   ```

2. Follow the setup instructions in the `README.md` of each component.

## Usage

The announcements are plain DNS-SD, so any DNS-SD tool can browse them. On
macOS:

```sh
dns-sd -B _agents._tcp
```

On Linux, with Avahi:

```sh
avahi-browse --resolve _agents._tcp
```

To announce an MCP server of your own, listening on port 8003 with its endpoint
at `/mcp`:

```sh
dns-sd -R notes _agents._tcp,_mcp local 8003 txtvers=1 proto=mcp path=/mcp proto-ver=2025-11-25
```

```sh
avahi-publish-service --subtype _mcp._sub._agents._tcp notes _agents._tcp 8003 txtvers=1 proto=mcp path=/mcp proto-ver=2025-11-25
```

See the [draft][draft] for the full set of TXT keys, and for A2A agents.

## Roadmap

See the [open issues](https://github.com/cisco-open/zeroconf-agent-discovery/issues)
for a list of proposed features (and known issues).

## Contributing

Contributions are what make the open source community such an amazing place to
learn, inspire, and create. Any contributions you make are **greatly
appreciated**. For detailed contributing guidelines, please see
[CONTRIBUTING.md](CONTRIBUTING.md)

## License

Distributed under the `Apache-2.0` License. See [LICENSE](LICENSE) for more
information.

## Contact

Lori Jakab - [@ljakab](https://github.com/ljakab)

Project Link:
[https://github.com/cisco-open/zeroconf-agent-discovery](https://github.com/cisco-open/zeroconf-agent-discovery)

## Acknowledgements

This template was adapted from
[https://github.com/othneildrew/Best-README-Template](https://github.com/othneildrew/Best-README-Template).

[draft]: https://datatracker.ietf.org/doc/draft-jakab-dawn-agent-discovery-mdns/
[rfc6762]: https://www.rfc-editor.org/rfc/rfc6762
[rfc6763]: https://www.rfc-editor.org/rfc/rfc6763
[python-zeroconf]: https://github.com/python-zeroconf/python-zeroconf
[codex]: https://github.com/openai/codex
[mdns-sd]: https://crates.io/crates/mdns-sd
[hackathon]: https://wiki.ietf.org/meeting/127/hackathon
