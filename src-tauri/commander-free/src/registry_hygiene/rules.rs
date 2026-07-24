pub(super) fn is_valid_clsid(value: &str) -> bool {
    value.len() == 38
        && value.starts_with('{')
        && value.ends_with('}')
        && value.chars().enumerate().all(|(index, c)| match index {
            0 | 37 => c == if index == 0 { '{' } else { '}' },
            9 | 14 | 19 | 24 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

pub(super) fn server_executable_path(value: &str) -> Option<String> {
    let value = value.trim();
    let token = if let Some(rest) = value.strip_prefix('"') {
        rest.split('"').next()?
    } else {
        value.split_whitespace().next()?
    };
    let expanded = expand_known_environment(token);
    if expanded.contains('%') || !is_windows_absolute_path(&expanded) {
        return None;
    }
    Some(expanded)
}

fn is_windows_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/'))
        || value.starts_with("\\\\")
}

fn expand_known_environment(value: &str) -> String {
    let mut result = value.to_string();
    for name in [
        "SystemRoot",
        "WINDIR",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "LOCALAPPDATA",
        "APPDATA",
    ] {
        if let Ok(replacement) = std::env::var(name) {
            result = result.replace(&format!("%{name}%"), &replacement);
            result = result.replace(&format!("%{}%", name.to_ascii_lowercase()), &replacement);
        }
    }
    result
}

pub(super) fn is_allowed_orphan(class_id: &str, server: &str) -> bool {
    is_valid_clsid(class_id)
        && server_executable_path(server).is_some_and(|path| !std::path::Path::new(&path).exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_guid_clsids() {
        assert!(is_valid_clsid("{01234567-89ab-cdef-0123-456789abcdef}"));
        assert!(!is_valid_clsid("not-a-clsid"));
    }
    #[test]
    fn refuses_relative_and_unexpanded_servers() {
        assert_eq!(server_executable_path("missing.dll"), None);
        assert_eq!(server_executable_path("%UNKNOWN%\\missing.dll"), None);
    }
    #[test]
    fn parses_quoted_server_commands() {
        assert_eq!(
            server_executable_path("\"C:\\No Space\\server.exe\" /automation"),
            Some("C:\\No Space\\server.exe".into())
        );
    }
}
