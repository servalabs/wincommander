// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native dialog selection. No renderer-supplied paths or session owners.
//! Integration must tie each session to its actual window and authenticated
//! sidecar connection, call close on disconnect/lock, and drive expiry.
//! The sidecar must independently compare the captured filesystem identity.
#![allow(dead_code)]
use crate::file_package_grants::{GrantStore, Owner, Purpose};
use crate::file_package_identity::{BoundSelection, Identity};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};
use tauri_plugin_dialog::DialogExt;
use wincmd_shared::file_package::PackageToken;

#[derive(Clone, Copy)]
pub(crate) enum PickKind {
    SourceFiles,
    SourceFolder,
    Carrier,
    Output,
    Destination,
}
impl PickKind {
    fn purpose(self) -> Purpose {
        match self {
            Self::SourceFiles => Purpose::SourceFile,
            Self::SourceFolder => Purpose::SourceDirectory,
            Self::Carrier => Purpose::Carrier,
            Self::Output => Purpose::Output,
            Self::Destination => Purpose::Destination,
        }
    }
}

#[derive(serde::Serialize)]
pub(crate) struct Picked {
    pub token: PackageToken,
    pub name: String,
}

#[derive(Default)]
pub(crate) struct SelectionManager {
    store: Arc<Mutex<GrantStore<Arc<BoundSelection>>>>,
    dialog_active: Arc<AtomicBool>,
}
impl SelectionManager {
    pub fn session(&self) -> Result<SelectionSession, &'static str> {
        Ok(SelectionSession {
            store: Arc::clone(&self.store),
            owner: Owner::random()?,
            dialog_active: Arc::clone(&self.dialog_active),
            closed: AtomicBool::new(false),
        })
    }
    pub fn expire(&self) -> Result<(), &'static str> {
        self.store
            .lock()
            .map_err(|_| "selection unavailable")?
            .expire(Instant::now());
        Ok(())
    }
}

pub(crate) struct SelectionSession {
    store: Arc<Mutex<GrantStore<Arc<BoundSelection>>>>,
    owner: Owner,
    dialog_active: Arc<AtomicBool>,
    closed: AtomicBool,
}
struct DialogGuard(Arc<AtomicBool>);
impl Drop for DialogGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl SelectionSession {
    /// The integration must supply the real owning main window, not a label
    /// from a renderer argument. A native picker decides every returned path.
    pub async fn pick(
        &self,
        app: &tauri::AppHandle,
        window: &tauri::WebviewWindow,
        kind: PickKind,
    ) -> Result<Vec<Picked>, &'static str> {
        if self.closed.load(Ordering::Acquire)
            || window.label() != "main"
            || crate::cli::tauri_runtime_active()
        {
            return Err("selection requires an interactive window");
        }
        self.dialog_active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "a selection dialog is already open")?;
        // The callback owns the guard: cancelling the waiting future does not
        // permit a second dialog while the original native picker is open.
        let guard = DialogGuard(Arc::clone(&self.dialog_active));
        let (send, receive) = tokio::sync::oneshot::channel();
        let picker = app.dialog().file().set_parent(window);
        match kind {
            PickKind::SourceFiles => picker.pick_files(move |value| {
                let _guard = guard;
                let _ = send.send(value);
            }),
            PickKind::SourceFolder | PickKind::Destination => picker.pick_folder(move |value| {
                let _guard = guard;
                let _ = send.send(value.map(|p| vec![p]));
            }),
            PickKind::Carrier => picker
                .add_filter("Image or video", &["png", "mp4"])
                .pick_file(move |value| {
                    let _guard = guard;
                    let _ = send.send(value.map(|p| vec![p]));
                }),
            PickKind::Output => picker
                .add_filter("Image or video", &["png", "mp4"])
                .save_file(move |value| {
                    let _guard = guard;
                    let _ = send.send(value.map(|p| vec![p]));
                }),
        }
        let Some(values) = receive
            .await
            .map_err(|_| "selection dialog closed unexpectedly")?
        else {
            return Ok(Vec::new());
        };
        let paths: Vec<PathBuf> = values
            .into_iter()
            .map(|value| {
                value
                    .into_path()
                    .map_err(|_| "choose a local file or folder")
            })
            .collect::<Result<_, _>>()?;
        self.register_native(paths, kind)
    }

    fn register_native(
        &self,
        paths: Vec<PathBuf>,
        kind: PickKind,
    ) -> Result<Vec<Picked>, &'static str> {
        if self.closed.load(Ordering::Acquire) {
            return Err("selection session closed");
        }
        if paths.is_empty() || paths.len() > crate::file_package_grants::MAX_BATCH {
            return Err("invalid selection count");
        }
        let names: Vec<_> = paths
            .iter()
            .map(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .ok_or("invalid selected name")
            })
            .collect::<Result<_, _>>()?;
        let bindings = paths
            .into_iter()
            .map(|path| BoundSelection::open(path, kind.purpose()).map(Arc::new))
            .collect::<Result<Vec<_>, _>>()?;
        let mut store = self.store.lock().map_err(|_| "selection unavailable")?;
        if self.closed.load(Ordering::Acquire) {
            return Err("selection session closed");
        }
        let tokens = store.issue(self.owner, kind.purpose(), bindings, Instant::now())?;
        Ok(tokens
            .into_iter()
            .zip(names)
            .map(|(token, name)| Picked { token, name })
            .collect())
    }

    /// Native only: authorization must already bind this session to the caller.
    pub fn resolve(&self, kind: PickKind, token: &PackageToken) -> Result<Resolved, &'static str> {
        let mut store = self.store.lock().map_err(|_| "selection unavailable")?;
        if self.closed.load(Ordering::Acquire) {
            return Err("selection session closed");
        }
        store
            .resolve(self.owner, kind.purpose(), token, Instant::now())
            .map(|selected| Resolved {
                selected: Arc::clone(selected),
            })
            .ok_or("selection expired or unavailable")
    }
    pub fn remove(&self, token: &PackageToken) -> bool {
        self.store
            .lock()
            .ok()
            .is_some_and(|mut store| store.revoke(self.owner, token))
    }
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        let mut store = self
            .store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        store.revoke_owner(self.owner);
    }
}

/// Never serialize this to the renderer; it is for the authenticated sidecar.
pub(crate) struct Resolved {
    selected: Arc<BoundSelection>,
}
impl Resolved {
    pub fn path(&self) -> &std::path::Path {
        self.selected.path()
    }
    pub fn identity(&self) -> Identity {
        self.selected.identity()
    }
}
impl Drop for SelectionSession {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
#[path = "file_package_picker_tests.rs"]
mod tests;
