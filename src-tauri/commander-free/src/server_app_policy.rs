// SPDX-License-Identifier: AGPL-3.0-or-later
use url::{Origin, Url};

pub(crate) fn validate_group(group: &str) -> Result<(), String> {
    match group {
        "server-app" | "productivity" | "mesh-login" => Ok(()),
        _ => Err("unsupported embedded-view group".into()),
    }
}

pub(crate) fn label(group: &str, id: &str) -> Result<String, String> {
    validate_group(group)?;
    if id.is_empty()
        || id.len() > 80
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("invalid embedded-view id".into());
    }
    Ok(format!("{group}-{id}"))
}

pub(crate) fn validate_url(url: &Url, dev_origin: Option<&Origin>) -> Result<(), String> {
    let host = url.host_str().unwrap_or("").trim_end_matches('.');
    // Windows maps native custom protocols to *.localhost. Never let an
    // external view (or its injected branding script) navigate into the app.
    if !matches!(url.scheme(), "http" | "https")
        || host.is_empty()
        || host.ends_with(".localhost")
        || !url.username().is_empty()
        || url.password().is_some()
        || dev_origin == Some(&url.origin())
    {
        return Err("embedded views require an external HTTP(S) URL without credentials".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_cannot_escape_ephemeral_storage_or_select_a_privileged_label() {
        for group in [
            "",
            "..",
            "../outside",
            r"..\outside",
            r"C:\outside",
            r"\\server\share",
            "notification",
            "main",
            "server-app/../main",
        ] {
            assert!(label(group, "login").is_err(), "{group}");
        }
        for group in ["server-app", "productivity", "mesh-login"] {
            assert_eq!(label(group, "app_1-2").unwrap(), format!("{group}-app_1-2"));
        }
    }

    #[test]
    fn view_ids_cannot_be_paths_or_unbounded_labels() {
        for id in ["", ".", "..", "a/b", r"a\b", "a:b", "a\0b", "a*b"] {
            assert!(label("server-app", id).is_err(), "{id}");
        }
        assert!(label("server-app", &"a".repeat(81)).is_err());
    }

    #[test]
    fn external_navigation_rejects_local_app_protocols_and_credentials() {
        for value in [
            "tauri://localhost/index.html",
            "asset://localhost/x",
            "file:///C:/test.html",
            "javascript:alert(1)",
            "data:text/html,test",
            "http://tauri.localhost/",
            "https://asset.localhost/",
            "https://TAURI.LOCALHOST./",
            "https://user:password@example.com/",
        ] {
            assert!(
                validate_url(&Url::parse(value).unwrap(), None).is_err(),
                "{value}"
            );
        }
    }

    #[test]
    fn self_hosted_apps_and_cross_origin_web_login_redirects_remain_available() {
        for value in [
            "http://localhost:5600/",
            "http://127.0.0.1:5600/",
            "http://[::1]:5600/",
            "https://example.ts.net/",
            "https://login.example.com/auth",
        ] {
            assert!(
                validate_url(&Url::parse(value).unwrap(), None).is_ok(),
                "{value}"
            );
        }
    }

    #[test]
    fn development_app_origin_is_not_an_external_app() {
        let dev = Url::parse("http://127.0.0.1:1420/").unwrap().origin();
        assert!(validate_url(
            &Url::parse("http://127.0.0.1:1420/index.html?x=1").unwrap(),
            Some(&dev)
        )
        .is_err());
        assert!(validate_url(&Url::parse("http://127.0.0.1:5600/").unwrap(), Some(&dev)).is_ok());
    }
}
