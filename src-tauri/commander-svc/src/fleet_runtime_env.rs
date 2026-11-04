// SPDX-License-Identifier: AGPL-3.0-or-later
//! Check that service networking cannot silently use a different proxy/CA setup.

pub(crate) fn fingerprint() -> Option<String> {
    let mut values = Vec::new();
    for key in wincmd_shared::fleet_transport_profile::ENV_KEYS {
        let value = match std::env::var_os(key) {
            None => None,
            Some(value) => Some(value.into_string().ok()?),
        };
        values.push(value);
    }
    let references: Vec<Option<&str>> = values.iter().map(|v| v.as_deref()).collect();
    wincmd_shared::fleet_transport_profile::fingerprint(&references)
}
