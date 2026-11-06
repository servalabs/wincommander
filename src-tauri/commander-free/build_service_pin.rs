// SPDX-License-Identifier: AGPL-3.0-or-later
pub fn validate(hash: Option<&str>, app_dir: Option<&str>) -> Result<(), &'static str> {
    let hash =
        hash.ok_or("paired encrypted-search release requires the final signed service SHA-256")?;
    if hash.len() != 64
        || !hash.bytes().all(|b| b.is_ascii_hexdigit())
        || !hash.bytes().any(|b| b != b'0')
    {
        return Err("invalid paired service SHA-256");
    }
    if app_dir.unwrap_or("WinCommander") != "WinCommander" {
        return Err("paired encrypted-search release requires the matching app directory");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate;
    #[test]
    fn paired_release_requires_exact_digest_and_app_directory() {
        assert!(validate(None, None).is_err());
        assert!(validate(Some("00"), None).is_err());
        assert!(validate(Some(&"0".repeat(64)), None).is_err());
        assert!(validate(Some(&"z".repeat(64)), None).is_err());
        assert!(validate(Some(&"a".repeat(64)), Some("Other")).is_err());
        assert!(validate(Some(&"a".repeat(64)), None).is_ok());
        assert!(validate(Some(&"A".repeat(64)), Some("WinCommander")).is_ok());
    }
}
