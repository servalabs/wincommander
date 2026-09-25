// SPDX-License-Identifier: AGPL-3.0-or-later
//! Resolve configured folders against currently observed volumes before any I/O.

use crate::settings::{AppSettings, FileSearchSettings, PrivateSearchRoot};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use wincmd_search::index::path_in_roots;
use wincmd_volume::{inspect_path, mounted_private_volumes, VolumeInfo};

#[derive(Clone)]
pub(super) struct PrivateShard {
    pub volume: VolumeInfo,
    pub roots: Vec<PathBuf>,
}

#[derive(Clone)]
pub(super) struct SearchPlan {
    pub ordinary: Vec<PathBuf>,
    pub private: Vec<PrivateShard>,
    pub protected: Vec<PathBuf>,
    pub blocked: Vec<PathBuf>,
    pub generation: String,
}

pub(super) fn contains(path: &Path, roots: &[PathBuf]) -> bool {
    !roots.is_empty() && path_in_roots(&path.to_string_lossy(), roots)
}

pub(super) fn same_path(a: &Path, b: &Path) -> bool {
    contains(a, &[b.to_path_buf()]) && contains(b, &[a.to_path_buf()])
}

/// This list is deliberately retained when a root is removed: old histories and
/// provider results must not become public when a drive letter is reused.
pub(super) fn enroll(settings: &mut AppSettings) -> Result<(), String> {
    let fs = &mut settings.app.file_search;
    let mut bindings = fs.private_roots.clone();
    for root in &fs.roots {
        if bindings.iter().any(|b| same_path(root, &b.path)) {
            continue;
        }
        let Ok(volume) = inspect_path(root) else {
            continue;
        };
        if !volume.is_private {
            continue;
        }
        let normalized = root.to_string_lossy().replace('/', "\\");
        let normalized = normalized.strip_prefix(r"\\?\").unwrap_or(&normalized);
        let normalized = format!(
            "{}{}",
            normalized[..1].to_ascii_uppercase(),
            &normalized[1..]
        );
        let normalized = PathBuf::from(normalized);
        let relative_path = normalized
            .strip_prefix(&volume.root)
            .map_err(|_| "Private search requires the volume's direct drive path.".to_string())?
            .to_path_buf();
        bindings.push(PrivateSearchRoot {
            path: root.clone(),
            volume_root: volume.root,
            relative_path,
            volume_id: volume.stable_id,
        });
    }
    if bindings != fs.private_roots {
        *settings = crate::settings::patch_settings(serde_json::json!({
            "app": { "fileSearch": { "private_roots": bindings } }
        }))?;
    }
    Ok(())
}

pub(super) fn plan(fs: &FileSearchSettings) -> Result<SearchPlan, String> {
    let mounted = mounted_private_volumes()?;
    Ok(resolve(fs, &mounted, inspect_path))
}

fn resolve(
    fs: &FileSearchSettings,
    mounted: &[VolumeInfo],
    inspect: impl Fn(&Path) -> Result<VolumeInfo, String>,
) -> SearchPlan {
    let mut ordinary = Vec::new();
    let mut private: Vec<PrivateShard> = Vec::new();
    let mut protected: Vec<_> = fs
        .private_roots
        .iter()
        .map(|b| b.volume_root.clone())
        .collect();
    protected.extend(mounted.iter().map(|v| v.root.clone()));
    let mut blocked = Vec::new();
    for root in &fs.roots {
        if let Some(binding) = fs.private_roots.iter().find(|b| same_path(&b.path, root)) {
            let Some(volume) = mounted.iter().find(|v| v.stable_id == binding.volume_id) else {
                blocked.push(binding.volume_root.clone());
                continue;
            };
            let resolved = volume.root.join(&binding.relative_path);
            if !safe_relative(&binding.relative_path)
                || inspect(&resolved).map_or(true, |v| v.identity != volume.identity)
            {
                blocked.push(volume.root.clone());
                continue;
            }
            protected.push(volume.root.clone());
            if let Some(shard) = private
                .iter_mut()
                .find(|s| s.volume.identity == volume.identity)
            {
                shard.roots.push(resolved);
            } else {
                private.push(PrivateShard {
                    volume: volume.clone(),
                    roots: vec![resolved],
                });
            }
        } else if !contains(root, &protected) && inspect(root).is_ok_and(|v| !v.is_private) {
            ordinary.push(root.clone());
        } else {
            blocked.push(root.clone());
        }
    }
    protected.sort();
    protected.dedup();
    blocked.sort();
    blocked.dedup();
    let mut digest = Sha256::new();
    for root in &fs.roots {
        digest.update(root.to_string_lossy().as_bytes());
        digest.update([0]);
    }
    for binding in &fs.private_roots {
        digest.update(binding.volume_id.as_bytes());
    }
    for exclusion in &fs.exclusions {
        digest.update(exclusion.as_bytes());
        digest.update([0]);
    }
    for volume in mounted {
        digest.update(volume.identity.as_bytes());
        digest.update(volume.root.to_string_lossy().as_bytes());
    }
    for root in &blocked {
        digest.update(root.to_string_lossy().as_bytes());
    }
    SearchPlan {
        ordinary,
        private,
        protected,
        blocked,
        generation: hex::encode(digest.finalize()),
    }
}

fn safe_relative(path: &Path) -> bool {
    path.components()
        .all(|c| matches!(c, std::path::Component::Normal(_)))
}

pub(super) fn ordinary_allowed(path: &Path, protected: &[PathBuf]) -> bool {
    !contains(path, protected) && inspect_path(path).is_ok_and(|v| !v.is_private)
}

pub(super) fn scope_roots(scope: Option<&str>, allowed: &[PathBuf]) -> Vec<PathBuf> {
    let Some(scope) = scope else {
        return allowed.to_vec();
    };
    let scope = PathBuf::from(scope);
    allowed
        .iter()
        .filter_map(|root| {
            if contains(&scope, &[root.clone()]) {
                Some(scope.clone())
            } else if contains(root, &[scope.clone()]) {
                Some(root.clone())
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_scope_cannot_restore_a_removed_root() {
        assert!(scope_roots(Some(r"V:\Secrets"), &[PathBuf::from(r"C:\Public")]).is_empty());
        assert!(scope_roots(Some(r"V:\Secrets"), &[]).is_empty());
    }

    #[test]
    fn broad_scope_intersects_authorized_folders() {
        let allowed = vec![PathBuf::from(r"V:\Allowed")];
        assert_eq!(scope_roots(Some(r"V:\"), &allowed), allowed);
        assert_eq!(
            scope_roots(Some(r"V:\Allowed\Child"), &allowed),
            vec![PathBuf::from(r"V:\Allowed\Child")]
        );
        assert!(scope_roots(Some(r"V:\AllowedSibling"), &allowed).is_empty());
    }

    #[test]
    fn empty_permission_set_never_matches_a_path() {
        assert!(!contains(Path::new(r"V:\secret.txt"), &[]));
    }

    #[test]
    fn relative_private_roots_cannot_escape_the_volume() {
        assert!(!safe_relative(Path::new("../escape")));
        assert!(!safe_relative(Path::new(r"C:\escape")));
        assert!(safe_relative(Path::new("Documents")));
        assert!(safe_relative(Path::new("")));
    }

    fn volume(root: &str, id: &str, generation: &str) -> VolumeInfo {
        VolumeInfo {
            root: PathBuf::from(root),
            stable_id: id.into(),
            identity: generation.into(),
            device: r"\Device\VeraCryptVolume1".into(),
            is_private: true,
            read_only: false,
        }
    }

    fn bound_settings() -> FileSearchSettings {
        FileSearchSettings {
            roots: vec![PathBuf::from(r"V:\Docs")],
            private_roots: vec![PrivateSearchRoot {
                path: PathBuf::from(r"V:\Docs"),
                volume_root: PathBuf::from(r"V:\"),
                relative_path: PathBuf::from("Docs"),
                volume_id: "vault-a".into(),
            }],
            ..FileSearchSettings::default()
        }
    }

    #[test]
    fn a_different_container_at_the_same_letter_is_blocked() {
        let impostor = volume(r"V:\", "vault-b", "b:1");
        let plan = resolve(&bound_settings(), &[impostor.clone()], |_| {
            Ok(impostor.clone())
        });
        assert!(plan.private.is_empty());
        assert!(plan.ordinary.is_empty());
        assert_eq!(plan.blocked, vec![PathBuf::from(r"V:\")]);
    }

    #[test]
    fn the_same_container_can_move_letters_without_authorizing_the_old_drive() {
        let mounted = volume(r"W:\", "vault-a", "a:2");
        let plan = resolve(&bound_settings(), &[mounted.clone()], |_| {
            Ok(mounted.clone())
        });
        assert_eq!(plan.private.len(), 1);
        assert_eq!(plan.private[0].roots, vec![PathBuf::from(r"W:\Docs")]);
        assert!(plan.protected.contains(&PathBuf::from(r"V:\")));
        assert!(plan.protected.contains(&PathBuf::from(r"W:\")));
    }

    #[test]
    fn locking_or_remounting_changes_the_generation_and_retains_history_protection() {
        let first = volume(r"V:\", "vault-a", "a:1");
        let second = volume(r"V:\", "vault-a", "a:2");
        let one = resolve(&bound_settings(), &[first.clone()], |_| Ok(first.clone()));
        let two = resolve(&bound_settings(), &[second.clone()], |_| Ok(second.clone()));
        let locked = resolve(&bound_settings(), &[], |_| Err("missing".into()));
        assert_ne!(one.generation, two.generation);
        assert_ne!(one.generation, locked.generation);
        assert!(locked.private.is_empty());
        assert!(locked.protected.contains(&PathBuf::from(r"V:\")));
    }
}
