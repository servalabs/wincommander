// SPDX-License-Identifier: AGPL-3.0-or-later

use std::path::{Path, PathBuf};

pub(super) struct StagedFile(pub PathBuf);

impl StagedFile {
    pub(super) fn new(target: &Path) -> Self {
        Self(target.with_extension(format!("{}.tmp", uuid::Uuid::new_v4())))
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(windows)]
fn replace(target: &Path, replacement: &Path, backup: Option<&Path>) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, ReplaceFileW, MOVEFILE_WRITE_THROUGH,
    };
    let wide = |path: &Path| {
        path.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let to = wide(target);
    let from = wide(replacement);
    let saved = backup.map(wide);
    let result = unsafe {
        if target.exists() {
            ReplaceFileW(
                to.as_ptr(),
                from.as_ptr(),
                saved
                    .as_ref()
                    .map_or(std::ptr::null(), |value| value.as_ptr()),
                0,
                std::ptr::null(),
                std::ptr::null(),
            )
        } else {
            MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH)
        }
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace(target: &Path, replacement: &Path, backup: Option<&Path>) -> std::io::Result<()> {
    if let Some(backup) = backup.filter(|_| target.exists()) {
        std::fs::copy(target, backup)?;
    }
    std::fs::rename(replacement, target)
}

pub(super) fn replace_with_metadata(
    target: &Path,
    staged: &Path,
    write_metadata: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    replace_with_metadata_using(target, staged, write_metadata, replace)
}

fn replace_with_metadata_using(
    target: &Path,
    staged: &Path,
    write_metadata: impl FnOnce() -> Result<(), String>,
    mut replace_file: impl FnMut(&Path, &Path, Option<&Path>) -> std::io::Result<()>,
) -> Result<(), String> {
    let backup = target.with_extension(format!("{}.previous", uuid::Uuid::new_v4()));
    let had_previous = target.exists();
    if let Err(error) = replace_file(target, staged, had_previous.then_some(backup.as_path())) {
        // ReplaceFileW may move the old image to backup before reporting failure.
        if had_previous && backup.exists() {
            return match replace_file(target, &backup, None) {
                Ok(()) => Err(format!("disk:Pro replacement failed: {error}. The previous Pro binary was restored.")),
                Err(rollback) => Err(format!("disk:Pro replacement failed: {error}. Restoration failed: {rollback}. The backup remains at {}. Repair Pro before retrying.", backup.display())),
            };
        }
        return Err(format!("disk:Pro replacement failed: {error}. Installation was not confirmed; retry the Pro update."));
    }
    if let Err(error) = write_metadata() {
        let restored = if had_previous {
            replace_file(target, &backup, None)
        } else {
            std::fs::remove_file(target)
        };
        return match restored {
            Ok(()) => Err(format!(
                "{error}. The previous Pro installation was restored."
            )),
            Err(rollback) => Err(format!(
                "{error}. Pro rollback failed: {rollback}. The backup was preserved at {}.",
                backup.display()
            )),
        };
    }
    if had_previous {
        let _ = std::fs::remove_file(backup);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> (PathBuf, PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("pro-replace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let target = dir.join("pro.exe");
        let staged = dir.join("pro.tmp");
        std::fs::write(&target, b"previous").unwrap();
        std::fs::write(&staged, b"verified replacement").unwrap();
        (dir, target, staged)
    }

    #[test]
    fn metadata_failure_restores_the_previous_binary() {
        let (dir, target, staged) = files();
        assert!(replace_with_metadata(&target, &staged, || Err("metadata denied".into())).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"previous");
        std::fs::remove_file(target).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn replacement_commits_only_after_metadata_and_cleans_backup() {
        let (dir, target, staged) = files();
        replace_with_metadata(&target, &staged, || {
            assert_eq!(std::fs::read(&target).unwrap(), b"verified replacement");
            Ok(())
        })
        .unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_file(target).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn failed_replacement_keeps_the_old_image_and_does_not_publish_metadata() {
        let (dir, target, staged) = files();
        std::fs::remove_file(&staged).unwrap();
        assert!(replace_with_metadata(&target, &staged, || panic!("must not publish")).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"previous");
        std::fs::remove_file(target).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn partial_replace_failure_restores_backup_before_returning_error() {
        let (dir, target, staged) = files();
        let mut first = true;
        let result = replace_with_metadata_using(
            &target,
            &staged,
            || panic!("must not publish"),
            |to, from, backup| {
                if first {
                    first = false;
                    std::fs::rename(to, backup.unwrap()).unwrap();
                    Err(std::io::Error::from_raw_os_error(1177))
                } else {
                    replace(to, from, backup)
                }
            },
        );
        assert!(result.unwrap_err().contains("restored"));
        assert_eq!(std::fs::read(&target).unwrap(), b"previous");
        std::fs::remove_file(staged).unwrap();
        std::fs::remove_file(target).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn replacement_succeeds_while_exclusive_maintenance_lease_is_held() {
        let (dir, target, staged) = files();
        let maintenance = super::super::update_guard::Maintenance::begin(&target)
            .await
            .unwrap();
        replace_with_metadata(&target, &staged, || Ok(())).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"verified replacement");
        drop(maintenance);
        std::fs::remove_file(target).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn failed_rollback_preserves_the_backup_and_reports_repair_required() {
        let (dir, target, staged) = files();
        let mut saved = None;
        let result = replace_with_metadata_using(
            &target,
            &staged,
            || panic!("must not publish"),
            |to, _, backup| {
                if let Some(backup) = backup {
                    std::fs::rename(to, backup).unwrap();
                    saved = Some(backup.to_path_buf());
                }
                Err(std::io::Error::from_raw_os_error(5))
            },
        );
        assert!(result.unwrap_err().contains("Repair Pro before retrying"));
        let backup = saved.unwrap();
        assert_eq!(std::fs::read(&backup).unwrap(), b"previous");
        std::fs::remove_file(backup).unwrap();
        std::fs::remove_file(staged).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn locked_destination_keeps_the_previous_binary_and_skips_metadata() {
        use std::os::windows::fs::OpenOptionsExt;
        let (dir, target, staged) = files();
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&target)
            .unwrap();
        assert!(replace_with_metadata(&target, &staged, || panic!("must not publish")).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"previous");
        drop(lock);
        std::fs::remove_file(staged).unwrap();
        std::fs::remove_file(target).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
