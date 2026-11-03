// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fleet destinations authorized during machine enrollment.

/// Normalize only the explicitly configured origins. The first may use the
/// existing HTTP transport; optional failovers require HTTPS as in the agent.
pub(crate) fn normalize(origins: &[String]) -> Result<Vec<String>, &'static str> {
    if origins.len() > 8 {
        return Err("too_many_origins");
    }
    let mut result = Vec::new();
    for (index, raw) in origins.iter().enumerate() {
        let parsed = url::Url::parse(raw).map_err(|_| "invalid_origin")?;
        if !matches!(parsed.scheme(), "http" | "https")
            || (index > 0 && parsed.scheme() != "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.path() != "/"
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err("invalid_origin");
        }
        let canonical = parsed.origin().ascii_serialization();
        if !result.contains(&canonical) {
            result.push(canonical);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_configured_origins_are_canonicalized_and_failovers_require_https() {
        assert_eq!(
            normalize(&[
                "http://Fleet.Example:80/".into(),
                "https://mesh.example/".into(),
                "https://mesh.example".into()
            ])
            .unwrap(),
            vec!["http://fleet.example", "https://mesh.example"]
        );
        for invalid in [
            "https://user@example.test",
            "https://host.test/path",
            "https://host.test/?x=1",
            "https://host.test/#x",
            "file:///tmp/a",
        ] {
            assert!(normalize(&[invalid.into()]).is_err());
        }
        assert!(normalize(&["https://one.test".into(), "http://two.test".into()]).is_err());
        assert!(normalize(&vec!["https://one.test".into(); 9]).is_err());
        assert!(normalize(&[]).unwrap().is_empty());
    }
}
