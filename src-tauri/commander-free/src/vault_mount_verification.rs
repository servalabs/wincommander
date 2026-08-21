// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 ServaLabs Pvt Ltd. See LICENSE for terms.
//
// The Pro engine verifies its own logon session after mounting. This companion
// command runs in the desktop process itself, which is the same Windows session
// as the user's File Explorer. It prevents a service/session-0 mount from being
// presented as usable to the signed-in person.

use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultDriveVerification {
    pub drive: String,
    pub accessible: bool,
}

fn drive_root(raw: &str) -> Result<(char, String), String> {
    let trimmed = raw.trim().strip_suffix(':').unwrap_or(raw.trim());
    let mut chars = trimmed.chars();
    let letter = chars
        .next()
        .filter(char::is_ascii_alphabetic)
        .ok_or_else(|| "Drive must be one letter from A through Z".to_string())?
        .to_ascii_uppercase();
    if chars.next().is_some() {
        return Err("Drive must be one letter from A through Z".to_string());
    }
    Ok((letter, format!("{letter}:\\")))
}

#[tauri::command]
pub fn verify_vault_drive(drive: String) -> Result<VaultDriveVerification, String> {
    let (letter, root) = drive_root(&drive)?;
    let metadata = std::fs::metadata(Path::new(&root)).map_err(|error| {
        format!("Drive {letter}: is not available in this signed-in Windows session: {error}")
    })?;
    if !metadata.is_dir() {
        return Err(format!(
            "Drive {letter}: is not available as an encrypted-volume root in this signed-in Windows session"
        ));
    }
    Ok(VaultDriveVerification {
        drive: format!("{letter}:"),
        accessible: true,
    })
}

#[cfg(test)]
mod tests {
    use super::drive_root;

    #[test]
    fn accepts_one_drive_letter_only() {
        assert_eq!(drive_root("q:").unwrap(), ('Q', "Q:\\".to_string()));
        assert!(drive_root("QQ").is_err());
        assert!(drive_root("C:\\").is_err());
        assert!(drive_root("C::").is_err());
        assert!(drive_root("").is_err());
    }
}
