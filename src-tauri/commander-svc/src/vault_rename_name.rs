//! The new file name a rename may give a Vault: a name only, never a folder,
//! that Windows can create and that names no device.

use std::path::Path;

const MAX_NAME_UTF16_UNITS: usize = 255;

fn is_reserved_device_name(name: &str) -> bool {
    // Windows reserves the device name with any extension and trailing spaces: "con .txt" too.
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ').to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") {
        return true;
    }
    let mut chars = stem.chars();
    let last = chars.next_back();
    matches!(chars.as_str(), "COM" | "LPT") && matches!(last, Some('0'..='9' | '¹' | '²' | '³'))
}

fn valid_file_name(name: &str) -> bool {
    let units = name.encode_utf16().count();
    (1..=MAX_NAME_UTF16_UNITS).contains(&units)
        && name != "."
        && name != ".."
        && !name.ends_with(['.', ' '])
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        && !is_reserved_device_name(name)
}

/// The file name to give `old_name`'s Vault, or the refusal code. A name with no
/// extension keeps the old one; an extensionless file stays extensionless.
pub(super) fn target_file_name(old_name: &str, new_name: &str) -> Result<String, &'static str> {
    if !valid_file_name(new_name) {
        return Err("name_invalid");
    }
    let name = match (Path::new(new_name).extension(), Path::new(old_name).extension()) {
        (None, Some(extension)) => format!("{new_name}.{}", extension.to_string_lossy()),
        _ => new_name.to_owned(),
    };
    if !valid_file_name(&name) {
        return Err("name_invalid");
    }
    if name.to_lowercase() == old_name.to_lowercase() {
        return Err("name_unchanged");
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_without_an_extension_keeps_the_old_one_and_an_extensionless_file_stays_so() {
        assert_eq!(target_file_name("notes.hc", "Taxes"), Ok("Taxes.hc".into()));
        assert_eq!(target_file_name("notes.hc", "Taxes.vc"), Ok("Taxes.vc".into()));
        assert_eq!(target_file_name("test3", "Test three"), Ok("Test three".into()));
        assert_eq!(target_file_name("notes.hc", " leading space ok"), Ok(" leading space ok.hc".into()));
    }

    #[test]
    fn names_windows_cannot_create_or_that_name_a_device_are_invalid() {
        let too_long = "a".repeat(MAX_NAME_UTF16_UNITS + 1);
        let fits_only_without_extension = "a".repeat(MAX_NAME_UTF16_UNITS - 1);
        for name in [
            "", ".", "..", "dot.", "space ", r"sub\name", "sub/name", "c:name", "star*", "what?", "quote\"",
            "less<", "more>", "pipe|", "tab\tname", "nul\0", "bell\u{7}", "CON", "con.txt", "Nul ", "aux.hc",
            "COM1", "lpt9.hc", "COM¹", "CONIN$", too_long.as_str(), fits_only_without_extension.as_str(),
        ] {
            assert_eq!(target_file_name("notes.hc", name), Err("name_invalid"), "{name:?}");
        }
        let longest = "a".repeat(MAX_NAME_UTF16_UNITS);
        assert_eq!(target_file_name("test3", &longest), Ok(longest.clone()));
        for allowed in ["COM10", "CONSOLE", "LPT", "Taxes 2026", "Ünïcode ✓"] {
            assert!(target_file_name("test3", allowed).is_ok(), "{allowed:?}");
        }
    }

    #[test]
    fn the_same_name_in_any_case_is_unchanged() {
        for name in ["notes", "notes.hc", "NOTES", "Notes.HC"] {
            assert_eq!(target_file_name("notes.hc", name), Err("name_unchanged"), "{name:?}");
        }
    }
}
