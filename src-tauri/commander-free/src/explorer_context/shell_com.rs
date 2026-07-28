// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/explorer_context/shell_com.rs
//! The unsafe half of the Explorer-folder probe: enumerate open shell windows
//! over COM, resolve each one's current folder to a filesystem path, and hand
//! back the best candidate. Nothing in here may panic — no `unwrap`/`expect` on
//! any COM- or OS-derived value, every allocation the shell gives us is freed on
//! every exit path, and the COM apartment is balanced by an RAII guard so early
//! returns and unwinding cannot leak it. All policy (what counts as a real
//! directory, how candidates rank) lives in the parent module as pure,
//! unit-tested functions; this file only talks to Windows.

use super::{accept_folder_path, folder_label, rank_window, ExplorerFolder};
use windows::core::Interface;
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IServiceProvider,
    CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    IFolderView, IPersistFolder2, IShellBrowser, IShellWindows, SHGetPathFromIDListEx, ShellWindows,
    GPFIDL_DEFAULT, SID_STopLevelBrowser,
};
use windows_sys::Win32::Foundation::HWND as SysHwnd;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetForegroundWindow, GetTopWindow, GetWindow, GA_ROOT, GW_HWNDNEXT,
};

/// Sanity bound on the enumeration so a bogus `Count()` cannot make us walk
/// forever. Each window costs several cross-process COM round trips.
///
/// Honest trade-off: `IShellWindows` enumerates in creation order, so past this
/// cap we could in principle miss the frontmost window. It degrades to offering
/// a different REAL folder, never a wrong-shaped path, and 64 open Explorer
/// windows is already far outside normal use.
const MAX_SHELL_WINDOWS: i32 = 64;
/// Caps the Z-order walk in case the sibling chain is ever cyclic.
const MAX_ZORDER_WALK: usize = 4096;

/// RAII COM apartment. `Drop` runs `CoUninitialize` on EVERY exit path — early
/// return, `?`, or unwinding — which is the entire reason this is a guard rather
/// than a matched pair of calls.
struct ComApartment {
    /// False when `CoInitializeEx` reported `RPC_E_CHANGED_MODE`: we never
    /// joined an apartment, so balancing the count would tear down an apartment
    /// somebody else on this thread owns.
    owned: bool,
}

impl ComApartment {
    fn enter() -> Option<Self> {
        // SAFETY: no reserved pointer; a plain single-threaded-apartment init.
        // Reports every outcome as an HRESULT rather than trapping.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        if hr.is_ok() {
            // S_OK and S_FALSE (already initialised on this thread) both
            // increment the per-thread count, so both must be balanced.
            Some(Self { owned: true })
        } else if hr == RPC_E_CHANGED_MODE {
            // tokio reuses blocking threads, so an earlier task may have made
            // this one an MTA. Shell automation still marshals fine from there.
            Some(Self { owned: false })
        } else {
            None
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: balances exactly one successful CoInitializeEx made on
            // this same thread.
            unsafe { CoUninitialize() };
        }
    }
}

/// A by-value `VT_I4` VARIANT. It owns no allocation, so it needs no
/// `VariantClear` and cannot leak.
fn variant_i4(value: i32) -> VARIANT {
    let mut variant = VARIANT::default();
    // SAFETY: `VARIANT::default()` is all-zeroes, a valid VT_EMPTY in the
    // VARIANT_0_0 arm of the union; writing `vt` + `lVal` is the documented way
    // to build an integer VARIANT in place.
    unsafe {
        let inner = &mut *variant.Anonymous.Anonymous;
        inner.vt = VT_I4;
        inner.Anonymous.lVal = value;
    }
    variant
}

/// Normalises any HWND to its top-level frame. Applied to BOTH sides of the
/// ranking comparison so they are guaranteed to live in the same space.
///
/// KT: `IShellBrowser::GetWindow` returns an INNER CHILD of the Explorer frame,
/// not the frame — measured on Win11: browser HWND 0xb0a4a for a frame at
/// 0x140cc4. Comparing that against `GetForegroundWindow` (always a top-level
/// window) therefore never matches, and the whole foreground/Z-order preference
/// silently degrades to "whichever window IShellWindows enumerated first" —
/// which is not even Z-order: measured, index 0 was the BACKMOST of four
/// windows. `GA_ROOT` on the browser HWND reproduces `IWebBrowser2::HWND`
/// exactly (verified for all 4 windows), without paying for another cast.
fn root_frame(hwnd: SysHwnd) -> usize {
    if hwnd.is_null() {
        return 0;
    }
    // SAFETY: pure read of window-manager state; documented to return NULL
    // rather than fail, and NULL is handled.
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    if root.is_null() {
        hwnd as usize
    } else {
        root as usize
    }
}

/// The top-level window the user is actually looking at.
fn foreground_root() -> usize {
    // SAFETY: documented to return NULL (nothing in the foreground) rather than
    // fail; root_frame handles NULL.
    root_frame(unsafe { GetForegroundWindow() })
}

/// Top-level windows front-to-back; index 0 is frontmost.
fn zorder_snapshot() -> Vec<usize> {
    let mut ordered = Vec::new();
    // SAFETY: NULL asks for the desktop's first child. The walk ends when
    // GetWindow returns NULL, and MAX_ZORDER_WALK caps a corrupted chain.
    unsafe {
        let mut hwnd = GetTopWindow(std::ptr::null_mut());
        while !hwnd.is_null() && ordered.len() < MAX_ZORDER_WALK {
            ordered.push(hwnd as usize);
            hwnd = GetWindow(hwnd, GW_HWNDNEXT);
        }
    }
    ordered
}

fn pidl_to_path(pidl: *const ITEMIDLIST) -> Option<String> {
    // Shell paths are not bounded by MAX_PATH, and this API fails rather than
    // truncating, so a generous buffer is the fail-safe choice.
    let mut buffer = [0u16; 1024];
    // SAFETY: the binding passes our slice length as `cchpath`, so the call
    // cannot write past `buffer`.
    let resolved = unsafe { SHGetPathFromIDListEx(pidl, &mut buffer, GPFIDL_DEFAULT) };
    if !resolved.as_bool() {
        // This PC / Recycle Bin / Quick access / Control Panel / MTP devices:
        // the shell is telling us no filesystem path exists. Primary filter —
        // the parent module's string checks are belt-and-braces behind it.
        return None;
    }
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16(&buffer[..end]).ok()
}

/// One shell window -> (top-level frame HWND as usize, folder), or None if it is
/// not an Explorer view sitting on a real directory.
fn inspect_window(list: &IShellWindows, index: i32) -> Option<(usize, ExplorerFolder)> {
    let position = variant_i4(index);
    // SAFETY: `position` is a by-value VARIANT alive for the whole call and
    // Item() only reads it. A window closed since Count() yields an Err.
    let dispatch = unsafe { list.Item(&position) }.ok()?;

    // Legacy IE windows and third-party shell hosts also live in this
    // collection; their missing top-level browser is the filter.
    let provider: IServiceProvider = dispatch.cast().ok()?;
    // SAFETY: SID_STopLevelBrowser with IShellBrowser is the documented pairing;
    // a mismatch is an Err, never UB.
    let browser: IShellBrowser = unsafe { provider.QueryService(&SID_STopLevelBrowser) }.ok()?;

    // SAFETY: IOleWindow::GetWindow on a live browser; returns an HRESULT.
    // Normalised to the frame — see root_frame's KT note. `.0` is already a
    // `*mut c_void`, the exact type windows-sys uses for HWND.
    let frame = root_frame(unsafe { browser.GetWindow() }.ok()?.0);
    // SAFETY: the active view of the browser we just obtained.
    let view = unsafe { browser.QueryActiveShellView() }.ok()?;
    let folder_view: IFolderView = view.cast().ok()?;
    // SAFETY: GetFolder is generic over the requested interface and returns an
    // Err when the view's folder does not implement IPersistFolder2.
    let persist: IPersistFolder2 = unsafe { folder_view.GetFolder() }.ok()?;
    // SAFETY: hands back a CoTaskMemAlloc'd PIDL that we own from here on.
    let pidl = unsafe { persist.GetCurFolder() }.ok()?;
    if pidl.is_null() {
        return None;
    }

    // Resolve BEFORE freeing, then free unconditionally — no `?` may sit between
    // the allocation and the free.
    let resolved = pidl_to_path(pidl);
    // SAFETY: frees exactly the PIDL GetCurFolder allocated for us; it is never
    // touched again after this point.
    unsafe { CoTaskMemFree(Some(pidl as *const core::ffi::c_void)) };

    let path = accept_folder_path(&resolved?)?;
    let label = folder_label(&path);
    Some((frame, ExplorerFolder { path, label }))
}

pub(super) fn probe() -> Option<ExplorerFolder> {
    // Bound to this scope, NOT dropped immediately: the leading underscore keeps
    // the binding NAMED so Drop still runs at the end of `probe`. A bare `_`
    // would uninitialise COM before the first call.
    let _apartment = ComApartment::enter()?;

    // SAFETY: CLSID_ShellWindows is a well-known local-server class. A refusal
    // (shell not running, COM blocked by policy, cross-integrity activation
    // denied) surfaces as an Err, not UB.
    let list: IShellWindows =
        unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER) }.ok()?;

    // SAFETY: plain property read on the collection.
    let count = unsafe { list.Count() }.unwrap_or(0);
    if count <= 0 {
        // The case this whole command is built around: "no Explorer window open"
        // is deliberately indistinguishable from "COM unavailable".
        return None;
    }

    let foreground = foreground_root();
    let ordered = zorder_snapshot();

    let mut best: Option<(u64, ExplorerFolder)> = None;
    for index in 0..count.min(MAX_SHELL_WINDOWS) {
        let Some((frame, folder)) = inspect_window(&list, index) else {
            continue;
        };
        let rank = rank_window(frame, foreground, &ordered);
        if best.as_ref().is_none_or(|(current, _)| rank < *current) {
            best = Some((rank, folder));
        }
        if rank == 0 {
            // Exact foreground match; nothing else can beat it.
            break;
        }
    }
    best.map(|(_, folder)| folder)
}
