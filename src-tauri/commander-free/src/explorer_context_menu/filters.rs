use super::SHELL_ROOTS;

pub(super) fn is_allowed_shell_verb(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    SHELL_ROOTS.iter().any(|root| {
        let root = root.to_ascii_lowercase();
        let Some(verb) = lower.strip_prefix(&(root + "\\")) else {
            return false;
        };
        !verb.is_empty() && !verb.contains('\\') && !verb.contains("..")
    })
}

fn command_binary(value: &str) -> Option<String> {
    let value = value.trim();
    let binary = if let Some(rest) = value.strip_prefix('"') {
        rest.split('"').next()?
    } else {
        value.split_whitespace().next()?
    };
    (is_windows_absolute_path(binary) && binary.to_ascii_lowercase().ends_with(".exe"))
        .then(|| binary.to_string())
}

fn is_windows_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/'))
        || value.starts_with("\\\\")
}

pub(super) fn is_third_party_verb(label: &str, command: &str) -> bool {
    let whole = format!("{label} {command}").to_ascii_lowercase();
    let Some(binary) = command_binary(command) else {
        return false;
    };
    let binary = binary.to_ascii_lowercase();
    ![
        "wincommander",
        "\\windows\\",
        "explorer.exe",
        "shell32.dll",
        "rundll32.exe",
    ]
    .iter()
    .any(|blocked| whole.contains(blocked) || binary.contains(blocked))
}

pub(super) fn safe_disabled_id(value: &str) -> bool {
    value.len() == 32 && value.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allows_only_explicit_shell_verb_paths() {
        assert!(is_allowed_shell_verb(
            "Software\\Classes\\Directory\\shell\\Tool"
        ));
        assert!(!is_allowed_shell_verb(
            "Software\\Classes\\Directory\\shellex\\ContextMenuHandlers\\Tool"
        ));
    }
    #[test]
    fn protects_wincommander_and_windows_verbs() {
        assert!(!is_third_party_verb(
            "WinCommander erase",
            "\"C:\\Tools\\tool.exe\""
        ));
        assert!(!is_third_party_verb(
            "System",
            "C:\\Windows\\System32\\tool.exe"
        ));
    }
    #[test]
    fn requires_absolute_executable_commands() {
        assert!(is_third_party_verb("Tool", "\"C:\\Tools\\tool.exe\" /x"));
        assert!(!is_third_party_verb("Tool", "cmd.exe /c tool"));
    }
}
