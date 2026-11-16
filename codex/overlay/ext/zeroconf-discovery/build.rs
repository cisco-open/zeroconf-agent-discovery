// Copyright 2026 Cisco Systems, Inc. and its affiliates
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

//! Chooses the browser backend at build time, from `ZEROCONF_BACKEND`:
//! `mdns-sd`, the default, or `mdnsresponder`, which is macOS only. The
//! variable is read here rather than through a Cargo feature, because the
//! crate is a dependency of `codex-app-server`, whose features the build
//! doesn't control; changing it rebuilds only this crate and its dependents.

use std::env;
use std::env::VarError;

const VAR: &str = "ZEROCONF_BACKEND";

fn main() {
    println!("cargo::rerun-if-env-changed={VAR}");
    println!("cargo::rustc-check-cfg=cfg(zeroconf_backend, values(\"mdnsresponder\"))");
    let backend = match env::var(VAR) {
        Ok(value) => value,
        Err(VarError::NotPresent) => String::new(),
        Err(VarError::NotUnicode(_)) => {
            println!("cargo::error={VAR} is not valid UTF-8");
            return;
        }
    };
    match backend.as_str() {
        "" | "mdns-sd" => {}
        "mdnsresponder" => {
            if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
                println!("cargo::rustc-cfg=zeroconf_backend=\"mdnsresponder\"");
            } else {
                println!("cargo::error={VAR}=mdnsresponder builds for macOS only");
            }
        }
        other => println!(
            "cargo::error={VAR}={other} is unknown; use mdns-sd, the default, or mdnsresponder (macOS only)"
        ),
    }
}
