// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fresh sign-in storage does not depend on deleting a previous cookie store.
use std::path::{Path, PathBuf};
use url::{Origin, Url};

pub(crate) struct Profile {
    path: PathBuf,
    _temporary: Option<tempfile::TempDir>,
}

impl Profile {
    pub(crate) fn prepare(user_data: &Path, ephemeral: bool) -> Result<Self, String> {
        if !user_data.is_absolute() {
            return Err("A per-user browser data directory is required".into());
        }
        let root = user_data.join("WebView2");
        if !ephemeral {
            return Ok(Self {
                path: root.join("ServerApps"),
                _temporary: None,
            });
        }
        let root = root.join("Ephemeral");
        std::fs::create_dir_all(&root).map_err(|_| "Sign-in storage could not be created")?;
        let directory = tempfile::Builder::new()
            .prefix("signin-")
            .tempdir_in(root)
            .map_err(|_| "A fresh sign-in profile could not be created")?;
        Ok(Self {
            path: directory.path().to_owned(),
            _temporary: Some(directory),
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn navigation_guard(
        self,
        dev_origin: Option<Origin>,
    ) -> impl Fn(&Url) -> bool + Send + Sync + 'static {
        move |url| {
            // Retain the profile until the native view releases its callback.
            // Cleanup is best-effort on Windows; a leftover is never reused.
            let _profile_lease = &self;
            crate::server_app_policy::validate_url(url, dev_origin.as_ref()).is_ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_signin_does_not_reuse_or_delete_a_previous_profile() {
        let root = tempfile::tempdir().unwrap();
        let first = Profile::prepare(root.path(), true).unwrap();
        std::fs::write(first.path().join("Cookies"), "old session").unwrap();
        let second = Profile::prepare(root.path(), true).unwrap();
        assert_ne!(first.path(), second.path());
        assert!(!second.path().join("Cookies").exists());
        assert!(first.path().join("Cookies").exists());
    }

    #[test]
    fn persistent_apps_keep_the_same_per_user_directory() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            Profile::prepare(root.path(), false).unwrap().path(),
            root.path().join("WebView2").join("ServerApps")
        );
    }

    #[test]
    fn relative_storage_is_not_redirected_to_shared_temp() {
        assert!(Profile::prepare(Path::new("relative"), true).is_err());
    }

    #[test]
    fn callback_holds_the_profile_until_the_view_releases_it() {
        let root = tempfile::tempdir().unwrap();
        let profile = Profile::prepare(root.path(), true).unwrap();
        let path = profile.path().to_owned();
        let guard = profile.navigation_guard(None);
        assert!(path.exists());
        assert!(guard(&Url::parse("https://login.example.com/").unwrap()));
        assert!(!guard(&Url::parse("http://tauri.localhost/").unwrap()));
        drop(guard);
        assert!(!path.exists());
    }
}
