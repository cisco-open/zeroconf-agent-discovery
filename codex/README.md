# Codex CLI with Zeroconf discovery

This directory adds discovery of MCP servers over mDNS/DNS-SD to
[OpenAI Codex CLI][codex], as specified in
[draft-jakab-dawn-agent-discovery-mdns][draft]. It does not contain Codex
itself: `build.sh` downloads the upstream source at a pinned release, adds the
code from this directory, and builds the result on your machine. The project
publishes no binaries.

For what works so far, see the [status table](../README.md#codex-cli-status)
in the top-level `README.md`.

## Layout

| Path       | Contents                                                      |
| ---------- | ------------------------------------------------------------- |
| `UPSTREAM` | The pinned upstream release: its tag and its commit           |
| `overlay/` | New files, copied into upstream's `codex-rs/` before patching |
| `patches/` | Changes to upstream files, applied in order with `git am`     |
| `build.sh` | Fetches the source, applies the changes, and builds           |

## Requirements

- macOS or Linux, with `git` and [rustup](https://rustup.rs). `build.sh`
  installs the Rust toolchain that upstream pins, if it is missing.
- Network access to github.com, chromium.googlesource.com and crates.io.
- Disk and time, as measured on an Apple M4 Pro with 24 GB of memory:

| What                                          | Measured         |
| --------------------------------------------- | ---------------- |
| One-time dependency download, cached by Cargo | About 4.5 GB     |
| First build, after the download               | 9 minutes        |
| Build output (`dev-small` profile)            | 10 GB            |
| Peak memory during the build                  | 4 GB             |
| Rebuild with nothing changed                  | Under 10 seconds |

Most of the download is the source of V8, which Codex uses through a Git fork
of `rusty_v8`: Cargo clones it with its submodules, full history included.

## Building

```sh
codex/build.sh
```

This fetches, applies and builds. The last line it prints is the path of the
`codex` binary. Run `codex/build.sh help` for the individual steps. To build an
optimized binary instead, which takes longer:

```sh
CODEX_PROFILE=release codex/build.sh build
```

Everything goes to `codex/.build/` unless `CODEX_BUILD_DIR` names another
directory. Each `apply` resets the source tree there to the pinned release, so
do not edit files in it by hand.

## Running

`codex/build.sh path` prints where the binary is, for the same
`CODEX_BUILD_DIR` and `CODEX_PROFILE` as the build:

```sh
"$(codex/build.sh path)" --no-daemon
```

The binary identifies itself as a modified build: `--version` adds this
repository's commit, as in `codex-cli 0.167.0 (zeroconf-agent-discovery
<commit>)`, and the TUI header and `/status` read `OpenAI Codex (v0.167.0) ·
zeroconf-agent-discovery build`. It is not a build from OpenAI.

It does not check for Codex updates on startup: it is pinned to one upstream
release, and offering an upgrade to another would be misleading. To check
anyway, pass `-c check_for_update_on_startup=true`; setting it in
`config.toml` works too, but that file is shared with any other Codex on the
machine (see below). Upstream's configuration reference still gives `true` as
the default.

It keeps the OAuth credentials of MCP servers in `CODEX_HOME/.credentials.json`,
readable only by you, instead of in the system keyring. Upstream does the same
for its own local builds. On macOS, the Keychain would ask for permission
again after every build, because each build gets a new ad-hoc signature, so
even "Always Allow" lasts only until the next build. MCP servers you logged in
to with another Codex therefore show "authentication required" here until you
run `codex mcp login <name>` with this binary. To use the keyring anyway, set
`mcp_oauth_credentials_store = "keyring"`.

Servers announced on the local network appear in `/mcp` as `lan-<instance>`,
and `/mcp verbose` shows them as disabled: Codex connects to none of them on
its own. To try it, announce one from another terminal, on macOS:

```sh
dns-sd -R notes _agents._tcp,_mcp local 8003 txtvers=1 proto=mcp path=/mcp
```

`lan-notes` shows up in `/mcp` within a few seconds, and is gone once you stop
`dns-sd`. A server found after Codex started shows as `unknown (0 tools)`
instead of `disabled (0 tools)`: Codex has no connection status for it yet,
and still does not connect to it. When `config.toml` already has a server with
the same name, Codex keeps that entry instead.

The URLs use `https`, as the draft requires. For test servers without TLS, set
`CODEX_ZEROCONF_ALLOW_HTTP=1`, which uses `http` for hosts in `.local`. To run
the binary without discovery, set `CODEX_ZEROCONF=0`. Both are read when Codex
starts.

Always pass `--no-daemon`. Codex can run a shared background app-server, for
example one started by a Codex already installed on the machine, and the TUI
connects to a running one by default. It starts its own app-server only when
there is none. The discovery code runs inside the app-server, so without
`--no-daemon` the patched TUI can end up talking to an unpatched app-server,
and nothing it discovers on the network shows up.

The binary uses the same Codex home as any other Codex on the machine:
`CODEX_HOME`, or `~/.codex` when that is not set. It therefore shares the
login, the configuration, the configured MCP servers and the session history.
Codex tolerates a state database that a newer Codex has already migrated, so
sharing the home with a newer installed Codex works. To keep the two apart,
point `CODEX_HOME` at another existing directory, and log in again there.

## Changing the overlay or the patches

Edit in the build tree, with plain Git, and write the result back:

```sh
codex/build.sh workspace   # apply, then switch the build tree to a branch
# edit in <build dir>/src/codex-rs, test with codex/build.sh build,
# and commit on the branch as usual
codex/build.sh test        # run the unit tests of the overlay crate
codex/build.sh export      # write the overlay and the patches back here
```

The first `test` takes about two minutes, because Cargo compiles the
dependencies again for the overlay crate on its own; later runs take seconds.

`export` copies the overlay directories (`ext/zeroconf-discovery`) back to
`overlay/`, and turns every commit that changes anything else into one patch
in `patches/`. Unchanged commits produce byte-identical patches. `Cargo.lock`
is never exported: Cargo rewrites it on every build.

`apply`, and therefore `workspace`, refuses to reset a workspace that holds
work not yet exported; `apply --force` discards it.

## Licensing

Codex is licensed under the Apache License 2.0 by OpenAI; see its
[LICENSE][codex-license] and [NOTICE][codex-notice]. The overlay, the patches
and the scripts in this directory are licensed under the Apache License 2.0
too, copyright Cisco Systems, Inc. and its affiliates.

Each patch states its copyright and license in its message, with a
`SPDX-License-Identifier: Apache-2.0` line. Section 4(b) of the license
requires modified files to carry prominent notices of the change, so the
first patch in the series that changes an upstream file also adds a line at
the top of that file saying that Cisco modified it. Later patches to the same
file rely on that notice; if the patch that added it is dropped, the next
patch that changes the file adds it instead. No patch removes or alters an
upstream notice. New patches follow the same rules.

[codex]: https://github.com/openai/codex
[codex-license]: https://github.com/openai/codex/blob/main/LICENSE
[codex-notice]: https://github.com/openai/codex/blob/main/NOTICE
[draft]: https://datatracker.ietf.org/doc/draft-jakab-dawn-agent-discovery-mdns/
