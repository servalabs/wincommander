// SPDX-License-Identifier: AGPL-3.0-or-later

/// Display only: callers must already authorize and resolve the container.
/// Never use this lexical projection as file identity or access validation.
pub fn normalize_vault_display_path(path: &str) -> Option<String> {
    let path = path
        .strip_prefix(r"\??\")
        .or_else(|| path.strip_prefix(r"\\?\"))
        .unwrap_or(path);
    let bytes = path.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || bytes[2] != b'\\'
        || path
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '?' | '*' | '"' | '<' | '>' | '|'))
        || path[2..].contains(':')
        || path[3..]
            .split('\\')
            .any(|part| part == "." || part == "..")
    {
        return None;
    }
    let mut display = path.to_owned();
    display.replace_range(..1, &path[..1].to_ascii_uppercase());
    Some(display)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_only_recognized_absolute_drive_paths() {
        assert_eq!(
            normalize_vault_display_path(r"\??\D:\Vault\New folder\test5").as_deref(),
            Some(r"D:\Vault\New folder\test5")
        );
        for path in [
            r"D:\Vault\test5",
            r"\??\D:\Vault\test5",
            r"\\?\D:\Vault\test5",
            r"d:\Vault\test5",
        ] {
            assert_eq!(
                normalize_vault_display_path(path).as_deref(),
                Some(r"D:\Vault\test5")
            );
        }
        for path in [
            "???",
            r"\Device\HarddiskVolume2\vault",
            r"\\?\UNC\server\vault",
            r"\\server\vault",
            r"D:vault",
            r"D:\..\vault",
            r"D:\vault?",
            r"D:\vault:stream",
            "D:\\vault\n",
        ] {
            assert!(normalize_vault_display_path(path).is_none(), "{path:?}");
        }
    }
}
