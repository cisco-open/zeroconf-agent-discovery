#!/usr/bin/env bash
# Copyright 2026 Cisco Systems, Inc. and its affiliates
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#
# SPDX-License-Identifier: Apache-2.0

# Builds OpenAI Codex CLI from upstream source, with this repository's overlay
# and patches applied. See README.md next to this script.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source-path=SCRIPTDIR source=UPSTREAM
. "$here/UPSTREAM"

build_dir="${CODEX_BUILD_DIR:-$here/.build}"
src="$build_dir/src"
target="$build_dir/target"
profile="${CODEX_PROFILE:-dev-small}"

# Identity for the commits this script makes in the build tree, so that the
# build does not depend on the user's git configuration. Patch authors are
# preserved by `git am`; this only sets the committer.
git_id=(-c user.name=zeroconf-agent-discovery -c user.email=build@zeroconf-agent-discovery.invalid)

# Directories under codex-rs/ that belong to the overlay. On export, changes
# in them go back to overlay/; changes anywhere else become patches.
overlay_dirs=(ext/zeroconf-discovery)

# Cargo packages in the overlay, which `test` and `check` work on.
overlay_packages=(zeroconf-discovery)
overlay_args=()
for package in "${overlay_packages[@]}"; do
	overlay_args+=(-p "$package")
done

# Paths cargo regenerates during a build and that are never patched. Upstream
# commits Cargo.lock with its workspace crates at version 0.0.0, so every build
# rewrites it.
generated=(":(exclude)codex-rs/Cargo.lock")

# The branch in the build tree that `workspace` creates for editing.
workspace_branch=zeroconf

log() { printf '==> %s\n' "$*" >&2; }
die() {
	printf 'build.sh: %s\n' "$*" >&2
	exit 1
}

usage() {
	cat <<EOF
Usage: codex/build.sh [command]

Builds OpenAI Codex CLI $CODEX_TAG with this repository's overlay and patches.

Commands:
  fetch   Download the pinned upstream commit into the build directory
  apply   Reset the build tree to the pinned commit, then add the overlay
          and apply the patches. Refuses while the workspace holds work that
          is not exported; 'apply --force' discards it
  build   Compile the codex binary and print its path
  test    Run the unit tests of the overlay crates
  check   Lint the overlay crates with clippy, run their tests, and check
          that the codex binary compiles
  all     fetch, apply and build (the default)
  path    Print the path of the built codex binary, without building

  workspace  apply, then put the build tree on branch '$workspace_branch' for
             editing: commit changes there with git, as usual
  export     Write the workspace back: the overlay directories to overlay/,
             every other change to patches/, one patch per commit

Environment:
  CODEX_BUILD_DIR  Build directory, outside the repository if you like
                   (default: codex/.build)
  CODEX_PROFILE    Cargo profile: dev-small (default, smallest on disk)
                   or release (optimized, slower to build)
  ZEROCONF_BACKEND The browser for servers on the local network: mdns-sd
                   (default, its own mDNS stack) or mdnsresponder (macOS
                   only, the system's mDNSResponder). --version names a
                   backend other than the default
EOF
}

# Where cargo puts the binary for the selected profile. The dev profile is the
# one exception to "the directory is named after the profile".
binary_path() {
	local out="$profile"
	if [[ "$profile" == dev ]]; then
		out=debug
	fi
	printf '%s\n' "$target/$out/codex"
}

have_pin() {
	git -C "$src" cat-file -e "$CODEX_COMMIT^{commit}" 2>/dev/null
}

fetch() {
	if have_pin; then
		log "upstream $CODEX_TAG is already fetched"
		return
	fi
	log "fetching $CODEX_TAG ($CODEX_COMMIT)"
	if [[ ! -d "$src/.git" ]]; then
		mkdir -p "$src"
		git -C "$src" init -q
		git -C "$src" remote add origin "$CODEX_REPO"
	fi
	git -C "$src" fetch -q --depth 1 origin "$CODEX_COMMIT"
	have_pin || die "the fetched objects do not contain $CODEX_COMMIT"
}

in_workspace() {
	[[ "$(git -C "$src" symbolic-ref -q --short HEAD 2>/dev/null)" == "$workspace_branch" ]]
}

# Write one patch per workspace commit that changes anything outside the
# overlay directories. --zero-commit and -N keep unchanged patches
# byte-identical between exports.
write_patches() {
	local excludes=() d
	for d in "${overlay_dirs[@]}"; do
		excludes+=(":(exclude)codex-rs/$d")
	done
	git -C "$src" format-patch -q -N --zero-commit --no-signature -o "$1" \
		"$CODEX_COMMIT..HEAD" -- . "${excludes[@]}" "${generated[@]}"
}

# Uncommitted changes in the build tree, other than generated files.
uncommitted() {
	git -C "$src" status --porcelain -- . "${generated[@]}"
}

# True when the workspace holds something that export has not written yet.
unexported() {
	[[ -n "$(uncommitted)" ]] && return 0
	local tmp d differs=1
	tmp="$(mktemp -d)"
	write_patches "$tmp"
	diff -rq -x .gitkeep "$tmp" "$here/patches" >/dev/null || differs=0
	rm -rf "$tmp"
	for d in "${overlay_dirs[@]}"; do
		if [[ -d "$src/codex-rs/$d" || -d "$here/overlay/$d" ]]; then
			diff -rq "$src/codex-rs/$d" "$here/overlay/$d" >/dev/null 2>&1 || differs=0
		fi
	done
	return "$differs"
}

apply() {
	have_pin || die "run 'fetch' first"
	if [[ "${1:-}" != --force ]] && in_workspace && unexported; then
		die "the workspace holds work that is not exported; run 'export' first, or 'apply --force' to discard it"
	fi
	log "resetting the build tree to $CODEX_TAG"
	if [[ -d "$src/.git/rebase-apply" ]]; then
		git -C "$src" am --abort
	fi
	git -C "$src" checkout -q -f --detach "$CODEX_COMMIT"
	git -C "$src" branch -q -D "$workspace_branch" 2>/dev/null || true
	git -C "$src" clean -q -fdx

	if [[ -n "$(find "$here/overlay" -type f ! -name .gitkeep -print -quit 2>/dev/null)" ]]; then
		log "adding the overlay"
		tar -C "$here/overlay" --exclude .gitkeep -cf - . | tar -C "$src/codex-rs" -xf -
		git -C "$src" add -A codex-rs
		git -C "$src" "${git_id[@]}" commit -q -m "zeroconf-agent-discovery overlay"
	fi

	shopt -s nullglob
	local patches=("$here"/patches/*.patch)
	shopt -u nullglob
	if ((${#patches[@]} == 0)); then
		log "no patches to apply"
		return
	fi
	log "applying ${#patches[@]} patch(es)"
	if ! git -C "$src" "${git_id[@]}" am -q "${patches[@]}"; then
		git -C "$src" am --abort
		die "a patch does not apply to $CODEX_TAG; git named it above"
	fi
}

# Runs cargo with the given arguments in the build tree, with the toolchain
# that upstream pins and the selected profile.
run_cargo() {
	[[ -f "$src/codex-rs/Cargo.toml" ]] || die "run 'fetch' and 'apply' first"
	command -v rustup >/dev/null || die "rustup is required, see https://rustup.rs"
	local toolchain
	toolchain="$(sed -n 's/^channel *= *"\(.*\)"$/\1/p' "$src/codex-rs/rust-toolchain.toml")"
	[[ -n "$toolchain" ]] || die "no toolchain named in codex-rs/rust-toolchain.toml"

	log "cargo $1 with Rust $toolchain, profile $profile"
	rustup toolchain install --profile minimal "$toolchain" >/dev/null
	# Put the pinned toolchain's own bin directory first on PATH, so cargo,
	# rustc and every tool cargo calls by name come from it. `rustup run` is
	# not enough: it relies on rustup's proxies being on PATH, and Homebrew's
	# rustup keeps them off it.
	local bin
	bin="$(dirname "$(rustup which --toolchain "$toolchain" cargo)")"
	# The commit of this repository, shown by `codex --version`.
	local commit
	commit="$(git -C "$here" describe --always --dirty --abbrev=7 2>/dev/null || echo unknown)"
	# A backend other than the default follows the commit, under the name of
	# what it browses through, so that builds with different backends can be
	# told apart. build.rs rejects any other value.
	case "${ZEROCONF_BACKEND:-}" in
	mdnsresponder) commit+=", mDNSResponder" ;;
	esac
	# The fork of rusty_v8 links a prebuilt V8 variant (pointer compression and
	# sandbox) that denoland does not publish. OpenAI publishes it with the
	# Codex releases, and upstream's Bazel build fetches it from there with
	# checksums pinned in MODULE.bazel; point cargo at the same files and
	# checksum. With a mirror set, the build script never falls back to
	# denoland.
	local -a v8_env=()
	local v8_version v8_sha
	v8_version="$(awk '$0 == "name = \"v8\"" { getline; gsub(/^version = "|"$/, ""); print; exit }' \
		"$src/codex-rs/Cargo.lock")"
	if [[ -n "$v8_version" ]]; then
		v8_env=(
			RUSTY_V8_MIRROR=https://github.com/openai/codex/releases/download
			"RUSTY_V8_MIRROR_TAG=rusty-v8-v$v8_version"
		)
		v8_sha="$(v8_archive_sha256 "$v8_version" "$("$bin/rustc" -vV | sed -n 's/^host: //p')")"
		if [[ -n "$v8_sha" ]]; then
			v8_env+=("RUSTY_V8_ARCHIVE_SHA256=$v8_sha")
		else
			log "warning: MODULE.bazel pins no checksum for V8 $v8_version on this host"
		fi
	fi
	# Codex patches in a git fork of rusty_v8, whose submodules (V8, ICU, LLVM
	# libraries and more) cargo clones on the first build. Fetch them with the
	# system git, which follows the machine's proxy and certificate settings;
	# cargo's built-in git client drops connections on some corporate networks.
	(cd "$src/codex-rs" &&
		env PATH="$bin:$PATH" CARGO_TARGET_DIR="$target" CARGO_NET_GIT_FETCH_WITH_CLI=true \
			ZEROCONF_AGENT_DISCOVERY_COMMIT="$commit" ${v8_env[@]+"${v8_env[@]}"} \
			cargo "$1" --profile "$profile" "${@:2}")
}

# Prints the SHA-256 that upstream's MODULE.bazel pins for the prebuilt V8
# archive of the given V8 version and Rust target, or nothing.
v8_archive_sha256() {
	local name="rusty_v8_${1//./_}_${2//-/_}_archive"
	awk -v name="\"$name\"," '
		$1 == "name" && $3 == name { found = 1; next }
		found && $1 == "sha256" { gsub(/[",]/, "", $3); print $3; exit }
		found && /^\)/ { exit }
	' "$src/MODULE.bazel"
}

build() {
	# Code mode, in which the model calls tools, MCP tools included, from a
	# script, runs that script in a separate host binary, which Codex looks
	# for next to its own; the official release ships both.
	run_cargo build -p codex-cli --bin codex \
		-p codex-code-mode-host --bin codex-code-mode-host

	local bin_path
	bin_path="$(binary_path)"
	log "built $bin_path"
	printf '%s\n' "$bin_path"
}

test_overlay() {
	run_cargo test "${overlay_args[@]}"
}

check() {
	run_cargo clippy "${overlay_args[@]}" --tests -- -D warnings
	test_overlay
	run_cargo check -p codex-cli
}

path() {
	local bin_path
	bin_path="$(binary_path)"
	[[ -x "$bin_path" ]] || die "no binary at $bin_path; run codex/build.sh first"
	printf '%s\n' "$bin_path"
}

workspace() {
	apply "$@"
	git -C "$src" switch -q -c "$workspace_branch"
	log "workspace ready: edit in $src/codex-rs and commit on branch $workspace_branch, then run 'export'"
}

export_workspace() {
	in_workspace || die "no workspace; run 'workspace' first"
	[[ -z "$(uncommitted)" ]] || die "commit the changes in $src first"
	local d
	rm -f "$here"/patches/*.patch
	write_patches "$here/patches"
	for d in "${overlay_dirs[@]}"; do
		if [[ -d "$src/codex-rs/$d" ]]; then
			mkdir -p "$here/overlay/$d"
			rsync -a --delete "$src/codex-rs/$d/" "$here/overlay/$d/"
		else
			rm -rf "${here:?}/overlay/$d"
		fi
	done
	shopt -s nullglob
	local patches=("$here"/patches/*.patch)
	shopt -u nullglob
	log "exported ${#patches[@]} patch(es) and the overlay"
}

case "${1:-all}" in
fetch) fetch ;;
apply) apply "${2:-}" ;;
workspace) workspace "${2:-}" ;;
export) export_workspace ;;
build) build ;;
test) test_overlay ;;
check) check ;;
path) path ;;
all)
	# Separate statements, not `fetch && apply && build`: bash ignores
	# `set -e` inside a function called on the left of `&&`.
	fetch
	apply
	build
	;;
-h | --help | help) usage ;;
*)
	usage >&2
	exit 2
	;;
esac
