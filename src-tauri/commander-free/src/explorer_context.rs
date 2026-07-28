// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/explorer_context.rs
//! Fail-safe "what folder is the user looking at" probe, feeding the Ctrl+Space
//! search overlay's "in this folder" chip. WinCommander is not a file browser,
//! so there is no current folder to inherit — instead we ask the shell which
//! Explorer window the user was last in and offer THAT folder. This is garnish,
//! never load-bearing: no Explorer window, COM refused by policy, a wedged
//! shell, or a virtual location with no filesystem identity all collapse to the
//! same indistinguishable `Ok(None)`, so the search box can neither block nor
//! surface an error the user has no way to act on.

use serde::Serialize;
use std::path::Path;
use std::time::Duration;

/// A hung shell (Explorer mid-restart, a modal shell extension on the view)
/// must degrade to "no suggestion", not a frozen search box.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplorerFolder {
    pub path: String,
    pub label: String,
}

/// Real on-disk directories Explorer will happily display but that must never
/// become a search scope.
///
/// KT: matched per PATH COMPONENT, lowercased — never as a string prefix or a
/// `contains`. `starts_with("$Recycle.Bin")` would also swallow a user's
/// "$Recycle.Bin backup" sibling, and `contains` would reject any legitimate
/// folder whose name merely embeds one of these.
const REJECTED_COMPONENTS: &[&str] = &[
    "$recycle.bin",
    "system volume information",
    "$windows.~bt",
    "$windows.~ws",
    "$getcurrent",
    "config.msi",
];

/// `D:\Projects\wincommander` -> "wincommander"; `D:\` -> `D:\`.
///
/// KT: split on separators by hand rather than leaning on `Path::file_name` —
/// its drive-root/UNC-root behaviour is target-dependent, and these strings
/// come from the shell, not from the host filesystem's grammar.
#[cfg_attr(not(windows), allow(dead_code))]
fn folder_label(raw: &str) -> String {
    let trimmed = raw.trim();
    let body = trimmed.trim_end_matches(['\\', '/']);
    let bytes = body.as_bytes();
    // A drive root has no final component. "D:" reads as a typo in a chip, so
    // hand back the shape the user recognises.
    if bytes.len() == 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return format!("{body}\\");
    }
    match body.rsplit(['\\', '/']).find(|segment| !segment.is_empty()) {
        Some(name) => name.to_string(),
        None => trimmed.to_string(),
    }
}

/// True for anything Explorer can show that has no filesystem identity at all:
/// This PC, Recycle Bin, Quick access, Control Panel, Network, MTP devices and
/// third-party namespace extensions.
///
/// The shell itself is the primary filter — `SHGetPathFromIDListEx` returns
/// FALSE for these. This predicate is the belt-and-braces second pass, because
/// a caller hands the result straight to a search scope and a namespace moniker
/// there would either error or, worse, be silently reinterpreted.
#[cfg_attr(not(windows), allow(dead_code))]
fn is_virtual_shell_location(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return true;
    }
    // Every namespace extension carries a "::{CLSID}" segment.
    if trimmed.contains("::{") {
        return true;
    }
    // ftp://, http://, mtp:// and the WebDAV/Network monikers.
    if trimmed.contains("://") {
        return true;
    }
    // \\?\Volume{...} and \\.\PhysicalDrive0 name devices, not directories.
    if trimmed.starts_with(r"\\?\") || trimmed.starts_with(r"\\.\") {
        return true;
    }
    // Monikers like "shell:Downloads" or "search-ms:query=...". A colon
    // anywhere but index 1 means this is a scheme, not a drive letter.
    !matches!(trimmed.find(':'), None | Some(1))
}

/// The only two shapes a filesystem directory can arrive in: `X:\…` and
/// `\\host\share…`. A bare `\\host` is the shell's server node, not a folder.
#[cfg_attr(not(windows), allow(dead_code))]
fn has_filesystem_shape(raw: &str) -> bool {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix(r"\\") {
        let mut parts = rest.split('\\').filter(|part| !part.is_empty());
        return parts.next().is_some() && parts.next().is_some();
    }
    let bytes = trimmed.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

#[cfg_attr(not(windows), allow(dead_code))]
fn has_rejected_component(raw: &str) -> bool {
    raw.split(['\\', '/'])
        .any(|segment| REJECTED_COMPONENTS.contains(&segment.trim().to_ascii_lowercase().as_str()))
}

/// The full gate. Returns the path only if the filesystem can genuinely resolve
/// it as a directory right now.
#[cfg_attr(not(windows), allow(dead_code))]
fn accept_folder_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if !has_filesystem_shape(trimmed)
        || is_virtual_shell_location(trimmed)
        || has_rejected_component(trimmed)
    {
        return None;
    }
    // KT: disk touch LAST — it is the only step that can block (stat'ing an
    // unreachable UNC host), and `is_dir()` folds every io::Error into `false`,
    // so a permissions problem degrades instead of propagating.
    if !Path::new(trimmed).is_dir() {
        return None;
    }
    Some(trimmed.to_string())
}

/// Lower is better. An exact foreground match wins outright; otherwise the
/// Explorer window nearest the front of the Z-order wins.
///
/// `frame` and `foreground` must BOTH be top-level frame HWNDs, and `ordered`
/// must be top-level windows front-to-back — mixing a child HWND in here is
/// what makes the whole ranking silently inert (see `root_frame`).
///
/// KT: Z-order IS the "most recently active" signal — Windows raises a window
/// to the front when it is activated, so no hook or history tracking is needed.
/// It is also the only thing that works here, because `IShellWindows` hands
/// windows back in an order unrelated to recency (measured: creation order).
/// A frame absent from the snapshot (closed mid-walk) sorts last but stays
/// eligible; dropping it would lose the only candidate on a race.
#[cfg_attr(not(windows), allow(dead_code))]
fn rank_window(frame: usize, foreground: usize, ordered: &[usize]) -> u64 {
    if frame != 0 && frame == foreground {
        return 0;
    }
    match ordered.iter().position(|candidate| *candidate == frame) {
        Some(index) => index as u64 + 1,
        None => u64::MAX,
    }
}

// The unsafe COM half lives in its own file: this module stays readable and
// every policy decision above it stays pure and unit-tested.
#[cfg(windows)]
#[path = "explorer_context/shell_com.rs"]
mod shell_com;

#[cfg(not(windows))]
mod shell_com {
    /// WinCommander is Windows-only in practice, but the crate is still checked
    /// for other targets; "no shell to ask" is just another clean empty case.
    pub(super) fn probe() -> Option<super::ExplorerFolder> {
        None
    }
}

/// Offers the folder of the Explorer window the user was last looking at, or
/// `Ok(None)` when there is nothing trustworthy to offer. Never `Err` in
/// practice — the fallible signature exists so the caller's IPC contract stays
/// uniform, and so a future hard failure has somewhere to go.
#[tauri::command]
pub async fn get_foreground_explorer_folder() -> Result<Option<ExplorerFolder>, String> {
    // KT: COM must never touch a tokio worker thread. CoInitializeEx pins an
    // apartment to the OS thread, but an async task can be moved between
    // workers at any await point — and uninitialising an apartment on a thread
    // the runtime keeps reusing would poison every later COM user.
    // spawn_blocking gives us a thread we own for the apartment's whole life.
    let probe = tokio::task::spawn_blocking(shell_com::probe);

    match tokio::time::timeout(PROBE_TIMEOUT, probe).await {
        Ok(Ok(found)) => Ok(found),
        // The blocking task panicked despite the no-panic discipline above. A
        // search chip is not worth an error toast; degrade silently.
        Ok(Err(_join_error)) => Ok(None),
        // Wedged shell. The orphaned blocking thread finishes on its own —
        // spawn_blocking cannot be cancelled — we just stop waiting for it.
        Err(_elapsed) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_uses_final_component() {
        assert_eq!(folder_label(r"D:\Projects\wincommander"), "wincommander");
        assert_eq!(folder_label(r"C:\Users\Admin\Downloads\"), "Downloads");
        assert_eq!(folder_label(r"\\nas\media\photos"), "photos");
    }

    #[test]
    fn label_falls_back_to_the_root_when_there_is_no_component() {
        assert_eq!(folder_label(r"D:\"), r"D:\");
        assert_eq!(folder_label("d:"), r"d:\");
        // A UNC share root has no component either; the share name is what the
        // user calls the place.
        assert_eq!(folder_label(r"\\nas\media"), "media");
    }

    #[test]
    fn virtual_locations_are_rejected() {
        // This PC, Control Panel, Recycle Bin, Quick access — all CLSID nodes.
        assert!(is_virtual_shell_location(
            "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}"
        ));
        assert!(is_virtual_shell_location(
            r"C:\x\::{645FF040-5081-101B-9F08-00AA002F954E}"
        ));
        assert!(is_virtual_shell_location("shell:Downloads"));
        assert!(is_virtual_shell_location("search-ms:query=budget"));
        assert!(is_virtual_shell_location("ftp://example.invalid/pub"));
        assert!(is_virtual_shell_location(r"\\?\Volume{12345678-0000-0000-0000-000000000000}\"));
        assert!(is_virtual_shell_location(r"\\.\PhysicalDrive0"));
        assert!(is_virtual_shell_location(""));
        assert!(is_virtual_shell_location("   "));
        assert!(is_virtual_shell_location("C:\0evil"));
    }

    #[test]
    fn real_directories_survive_the_virtual_check() {
        assert!(!is_virtual_shell_location(r"C:\Users\Admin\Downloads"));
        assert!(!is_virtual_shell_location(r"\\nas\media\photos"));
        assert!(!is_virtual_shell_location(r"D:\"));
    }

    #[test]
    fn shape_check_requires_a_drive_or_a_full_unc_share() {
        assert!(has_filesystem_shape(r"C:\Users"));
        assert!(has_filesystem_shape(r"D:\"));
        assert!(has_filesystem_shape(r"\\nas\media"));
        assert!(!has_filesystem_shape(r"\\nas"));
        assert!(!has_filesystem_shape("Downloads"));
        assert!(!has_filesystem_shape(r"\Users\Admin"));
        assert!(!has_filesystem_shape("C:"));
    }

    #[test]
    fn rejected_components_match_whole_components_only() {
        assert!(has_rejected_component(r"C:\$Recycle.Bin\S-1-5-21"));
        assert!(has_rejected_component(r"D:\SYSTEM VOLUME INFORMATION"));
        // Sibling-prefix safety: these share a prefix with a rejected name but
        // are ordinary user folders and must survive.
        assert!(!has_rejected_component(r"D:\$Recycle.Bin backup"));
        assert!(!has_rejected_component(r"D:\Archive\$Recycle.Bin.old"));
        assert!(!has_rejected_component(r"D:\System Volume Information Notes"));
        assert!(!has_rejected_component(r"C:\config.msi.bak"));
    }

    #[test]
    fn accept_requires_an_existing_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path().to_string_lossy().into_owned();
        assert_eq!(accept_folder_path(&dir).as_deref(), Some(dir.as_str()));

        // A sibling that merely shares the prefix does not exist, so it must be
        // rejected even though the real directory next to it is fine.
        let sibling = format!("{dir}-sibling");
        assert!(accept_folder_path(&sibling).is_none());

        // A file is not a directory.
        let file = temp.path().join("note.txt");
        std::fs::write(&file, b"x").expect("write");
        assert!(accept_folder_path(&file.to_string_lossy()).is_none());
    }

    #[test]
    fn ranking_prefers_the_foreground_then_the_front_of_the_zorder() {
        let ordered = vec![0x30_usize, 0x10, 0x20];
        // Exact foreground match beats everything, even a back window.
        assert_eq!(rank_window(0x20, 0x20, &ordered), 0);
        // Otherwise frontmost wins: 0x30 is ahead of 0x10.
        assert!(rank_window(0x30, 0x99, &ordered) < rank_window(0x10, 0x99, &ordered));
        // Unknown windows sort last but stay eligible.
        assert_eq!(rank_window(0x77, 0x99, &ordered), u64::MAX);
        // A NULL HWND must never be treated as a foreground match.
        assert_eq!(rank_window(0, 0, &ordered), u64::MAX);
    }
}
