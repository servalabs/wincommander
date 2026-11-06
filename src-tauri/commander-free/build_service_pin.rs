// SPDX-License-Identifier: AGPL-3.0-or-later
pub fn lab_build_enabled(value: Option<&str>) -> bool {
    value == Some("1")
}

pub fn validate(
    hash: Option<&str>,
    app_dir: Option<&str>,
    staged_hash: Option<&str>,
    pro_hash: Option<&str>,
) -> Result<(), &'static str> {
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
    let staged = staged_hash.ok_or("paired service resource is missing")?;
    if !hash.eq_ignore_ascii_case(staged) {
        return Err("paired service hash does not match staged resource");
    }
    let pro =
        pro_hash.ok_or("paired encrypted-search release requires the final guarded Pro SHA-256")?;
    if pro.len() != 64
        || !pro.bytes().all(|b| b.is_ascii_hexdigit())
        || !pro.bytes().any(|b| b != b'0')
    {
        return Err("invalid paired Pro SHA-256");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{lab_build_enabled, validate};
    #[test]
    fn paired_release_requires_explicit_lab_opt_in() {
        assert!(lab_build_enabled(Some("1")));
        assert!(!lab_build_enabled(None));
        assert!(!lab_build_enabled(Some("true")));
    }
    #[test]
    fn paired_release_requires_exact_digest_and_app_directory() {
        let pro = "b".repeat(64);
        assert!(validate(None, None, None, Some(&pro)).is_err());
        assert!(validate(Some("00"), None, None, Some(&pro)).is_err());
        assert!(validate(Some(&"0".repeat(64)), None, None, Some(&pro)).is_err());
        assert!(validate(Some(&"z".repeat(64)), None, None, Some(&pro)).is_err());
        assert!(validate(Some(&"a".repeat(64)), Some("Other"), None, Some(&pro)).is_err());
        assert!(validate(Some(&"a".repeat(64)), None, None, Some(&pro)).is_err());
        assert!(validate(
            Some(&"a".repeat(64)),
            None,
            Some(&"b".repeat(64)),
            Some(&pro)
        )
        .is_err());
        assert!(validate(Some(&"a".repeat(64)), None, Some(&"a".repeat(64)), None).is_err());
        assert!(validate(
            Some(&"a".repeat(64)),
            None,
            Some(&"a".repeat(64)),
            Some(&"0".repeat(64))
        )
        .is_err());
        assert!(validate(
            Some(&"a".repeat(64)),
            None,
            Some(&"a".repeat(64)),
            Some(&pro)
        )
        .is_ok());
        assert!(validate(
            Some(&"A".repeat(64)),
            Some("WinCommander"),
            Some(&"a".repeat(64)),
            Some(&pro)
        )
        .is_ok());
    }
}
