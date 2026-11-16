// SPDX-License-Identifier: AGPL-3.0-or-later
use std::{
    env,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub(super) enum Binaries {
    Classic {
        server: PathBuf,
        afk: PathBuf,
        window: PathBuf,
    },
    Tauri(PathBuf),
}

pub(super) fn discover_binaries() -> Option<Binaries> {
    let mut roots = Vec::new();
    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        let local_app_data = PathBuf::from(local_app_data);
        roots.push(local_app_data.join("Programs").join("ActivityWatch"));
        roots.push(local_app_data.join("Programs").join("ActivityWatch-Tauri"));
        let winget_packages = local_app_data
            .join("Microsoft")
            .join("WinGet")
            .join("Packages");
        if let Ok(entries) = std::fs::read_dir(winget_packages) {
            for path in entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.to_ascii_lowercase().starts_with("activitywatch"))
                })
            {
                roots.push(path.join("activitywatch"));
                roots.push(path);
            }
        }
    }
    for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(program_files) = env::var_os(variable) {
            roots.push(PathBuf::from(&program_files).join("ActivityWatch"));
            roots.push(PathBuf::from(program_files).join("ActivityWatch-Tauri"));
        }
    }
    roots.into_iter().find_map(|root| binaries_at(&root))
}

fn binaries_at(root: &Path) -> Option<Binaries> {
    // The Tauri bundle owns its embedded server and watcher lifecycle.
    let tauri = root.join("aw-tauri.exe");
    if tauri.is_file() {
        return Some(Binaries::Tauri(tauri));
    }
    let server = [
        "aw-server/aw-server.exe",
        "aw-server-rust/aw-server-rust.exe",
        "aw-server-rust/aw-server.exe",
        "aw-server.exe",
    ]
    .into_iter()
    .map(|relative| root.join(relative))
    .find(|path| path.is_file())?;
    let afk = root.join("aw-watcher-afk/aw-watcher-afk.exe");
    let window = root.join("aw-watcher-window/aw-watcher-window.exe");
    (afk.is_file() && window.is_file()).then_some(Binaries::Classic {
        server,
        afk,
        window,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tauri_bundle_does_not_require_a_standalone_server_binary() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("aw-tauri.exe"), []).unwrap();
        assert!(matches!(binaries_at(root.path()), Some(Binaries::Tauri(_))));
    }

    #[test]
    fn classic_bundle_requires_both_watchers_before_starting() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("aw-server.exe"), []).unwrap();
        assert!(binaries_at(root.path()).is_none());
        for watcher in ["aw-watcher-afk", "aw-watcher-window"] {
            std::fs::create_dir(root.path().join(watcher)).unwrap();
            std::fs::write(root.path().join(watcher).join(format!("{watcher}.exe")), []).unwrap();
        }
        assert!(matches!(
            binaries_at(root.path()),
            Some(Binaries::Classic { .. })
        ));
    }
}
