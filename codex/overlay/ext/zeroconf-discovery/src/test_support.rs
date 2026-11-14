// Copyright 2026 Cisco Systems, Inc. and its affiliates
// SPDX-License-Identifier: Apache-2.0

//! Helpers shared by the unit tests.

/// TXT entries from `key=value` strings, in order. A string without `=` is a
/// key without a value.
pub(crate) fn txt(pairs: &[&str]) -> Vec<(String, Option<Vec<u8>>)> {
    pairs
        .iter()
        .map(|pair| match pair.split_once('=') {
            Some((key, value)) => (key.to_string(), Some(value.as_bytes().to_vec())),
            None => (pair.to_string(), None),
        })
        .collect()
}
