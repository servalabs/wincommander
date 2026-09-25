// SPDX-License-Identifier: AGPL-3.0-or-later
//! Everything's database is independent of the content index. Never treat a
//! provider hit (or its aggregate count) as evidence of an accessible public file.
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub(super) struct ObservedVolume {
    pub root: PathBuf,
    pub identity: String,
    pub is_private: bool,
}

pub(super) struct PublicSearchPolicy {
    protected: Vec<PathBuf>,
    generation: String,
    observed: BTreeMap<String, ObservedVolume>,
    changed: bool,
}

fn local_path(path: &Path) -> Option<String> {
    let text = path.to_str()?.replace('/', "\\");
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    let bytes = text.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1..3] != *b":\\" {
        return None;
    }
    if text.chars().any(char::is_control)
        || text[3..].split('\\').any(|part| {
            part == "." || part == ".." || part.ends_with(['.', ' ']) || part.contains(':')
        })
    {
        return None;
    }
    Some(text.trim_end_matches('\\').to_ascii_lowercase())
}

fn protected(path: &Path, roots: &[PathBuf]) -> bool {
    let Some(path) = local_path(path) else {
        return true;
    };
    roots.iter().any(|root| {
        let Some(root) = local_path(root) else {
            return true;
        };
        path == root || path.starts_with(&format!("{root}\\"))
    })
}

impl PublicSearchPolicy {
    pub(super) fn new(protected: Vec<PathBuf>, generation: String) -> Self {
        Self {
            protected,
            generation,
            observed: BTreeMap::new(),
            changed: false,
        }
    }

    /// `inspect` must verify native identity, existence and every ancestor's
    /// reparse attributes. Tests inject observations; production uses wincmd-volume.
    pub(super) fn allows(
        &mut self,
        path: &Path,
        inspect: impl FnOnce(&Path) -> Option<ObservedVolume>,
    ) -> bool {
        if protected(path, &self.protected) {
            return false;
        }
        let Some(volume) = inspect(path) else {
            return false;
        };
        if volume.is_private || protected(&volume.root, &self.protected) {
            return false;
        }
        let Some(root) = local_path(&volume.root) else {
            return false;
        };
        let Some(path) = local_path(path) else {
            return false;
        };
        if path != root && !path.starts_with(&format!("{root}\\")) {
            return false;
        }
        if let Some(previous) = self.observed.get(&root) {
            if previous.identity != volume.identity {
                self.changed = true;
                return false;
            }
        }
        self.observed.insert(root, volume);
        true
    }

    /// A result is usable only while both the retained private-root policy and
    /// every ordinary volume observed during the request are still the same.
    pub(super) fn unchanged(
        &self,
        roots: &[PathBuf],
        generation: &str,
        inspect: impl Fn(&Path) -> Option<ObservedVolume>,
    ) -> bool {
        !self.changed
            && self.generation == generation
            && self.protected == roots
            && self.observed.values().all(|previous| {
                inspect(&previous.root).is_some_and(|current| {
                    !current.is_private
                        && current.identity == previous.identity
                        && local_path(&current.root) == local_path(&previous.root)
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn volume(root: &str, identity: &str, is_private: bool) -> ObservedVolume {
        ObservedVolume {
            root: root.into(),
            identity: identity.into(),
            is_private,
        }
    }
    fn ordinary(_: &Path) -> Option<ObservedVolume> {
        Some(volume(r"C:\", "disk-1", false))
    }
    fn policy() -> PublicSearchPolicy {
        PublicSearchPolicy::new(vec![r"V:\".into()], "mount-1".into())
    }

    #[test]
    fn protected_scope_is_rejected_before_any_provider_or_path_probe() {
        let mut policy = policy();
        for path in [r"V:\", r"v:\secret.txt", r"\\?\V:\folder\file", "V:/secret"] {
            assert!(!policy.allows(Path::new(path), |_| panic!("protected path was probed")));
        }
    }

    #[test]
    fn rejects_network_and_ambiguous_paths_before_probing() {
        let mut policy = policy();
        for path in [
            r"\\server\share\file",
            r"C:relative",
            r"C:\..\secret",
            r"C:\file:stream",
            r"C:\name.\file",
        ] {
            assert!(!policy.allows(Path::new(path), |_| panic!("ambiguous path was probed")));
        }
    }

    #[test]
    fn failed_native_checks_and_unrecognized_private_mounts_are_never_public() {
        let mut policy = policy();
        // Missing/offline paths and reparse aliases produce no native observation.
        assert!(!policy.allows(Path::new(r"C:\missing"), |_| None));
        assert!(!policy.allows(Path::new(r"C:\junction\secret"), |_| None));
        assert!(!policy.allows(Path::new(r"W:\secret"), |_| Some(volume(
            r"W:\", "vault", true
        ))));
        assert!(policy.allows(Path::new(r"C:\public.txt"), ordinary));
    }

    #[test]
    fn containment_respects_folder_boundaries_and_native_volume_root() {
        let mut policy = PublicSearchPolicy::new(vec![r"C:\private".into()], "one".into());
        assert!(!policy.allows(Path::new(r"C:\private\secret"), ordinary));
        assert!(policy.allows(Path::new(r"C:\private-other\public"), ordinary));
        assert!(!policy.allows(Path::new(r"D:\public.txt"), ordinary));
    }

    #[test]
    fn policy_change_or_volume_replacement_discards_already_filtered_results() {
        let mut policy = policy();
        assert!(policy.allows(Path::new(r"C:\public.txt"), ordinary));
        let roots = vec![PathBuf::from(r"V:\")];
        assert!(policy.unchanged(&roots, "mount-1", ordinary));
        assert!(!policy.unchanged(&roots, "mount-2", ordinary));
        assert!(!policy.unchanged(&roots, "mount-1", |_| None));
        assert!(!policy.unchanged(&roots, "mount-1", |_| Some(volume(r"C:\", "disk-2", false))));
        assert!(!policy.unchanged(&roots, "mount-1", |_| Some(volume(r"C:\", "vault", true))));
        assert!(!policy.unchanged(&[PathBuf::from(r"C:\")], "mount-1", ordinary));
    }

    #[test]
    fn transient_drive_replacement_cannot_revive_earlier_results() {
        let mut policy = policy();
        assert!(policy.allows(Path::new(r"C:\first.txt"), ordinary));
        assert!(!policy.allows(Path::new(r"C:\second.txt"), |_| Some(volume(
            r"C:\", "disk-2", false
        ))));
        assert!(!policy.unchanged(&[PathBuf::from(r"V:\")], "mount-1", ordinary));
    }
}
