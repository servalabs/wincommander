// SPDX-License-Identifier: AGPL-3.0-or-later
// Helpers for command identifiers that must remain dispatchable without
// embedding Pro/destructive tokens contiguously in the Free binary.

pub fn join_parts(parts: &[&str]) -> String {
    let mut out = String::with_capacity(parts.iter().map(|p| p.len()).sum());
    for part in parts {
        for ch in part.chars() {
            if ch != '~' {
                out.push(ch);
            }
        }
    }
    out
}

pub fn matches_parts(value: &str, parts: &[&str]) -> bool {
    value == join_parts(parts)
}
