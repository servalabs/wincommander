// src/components/UpdaterStatus.tsx
//
// In-app update indicator. Mounts inside the Dashboard panel.
//
// The check/download scheduling now lives in Rust (src-tauri/.../updater.rs):
// a tokio task that runs regardless of window state, so it keeps checking
// across long uptimes and recovers when a machine that booted offline
// reconnects — neither of which the old dashboard-mounted JS setInterval
// guaranteed. This component just reflects the shared updater snapshot
// (see hooks/updaterStore.ts).
//
// When auto-update is ON, a staged update is handled by the combined
// UpdateFlowDialog (see App.tsx), so this banner deliberately renders nothing
// for the "staged" phase and only surfaces the manual "Update WinCommander"
// button for the "available" phase (auto-update OFF).

import { invoke } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { useRef, useState } from "react";
import { Icon, Spinner } from "@/components/ui/bp";
import { useUpdater } from "../hooks/updaterStore";

export default function UpdaterStatus() {
    const snapshot = useUpdater();
    const [installing, setInstalling] = useState(false);
    // Ref guard: blocks a second click that arrives before React re-renders
    // the button as disabled. useState alone isn't synchronous enough.
    const installingRef = useRef(false);

    // Manual install path — only reachable when auto-update is OFF (the Rust
    // scheduler emits "available" instead of downloading). app_install_update_doh
    // does its own fresh DoH check + download + install, then we relaunch.
    const handleManualInstall = async () => {
        if (installingRef.current) return;
        installingRef.current = true;
        setInstalling(true);
        try {
            await invoke("app_install_update_doh");
            await relaunch();
        } catch (err) {
            console.error("Failed to install update:", err);
            installingRef.current = false;
            setInstalling(false);
        }
    };

    if (snapshot.phase === "checking") {
        return (
            <div className="updater-status updater-status--checking">
                <Spinner size={12} />
                <span className="updater-status__label">Checking for updates…</span>
            </div>
        );
    }

    if (snapshot.phase === "downloading") {
        return (
            <div className="updater-status updater-status--checking">
                <Spinner size={12} />
                <span className="updater-status__label">
                    Downloading update{snapshot.version ? ` v${snapshot.version}` : ""}…
                </span>
            </div>
        );
    }

    if (snapshot.phase === "error") {
        // Rust auto-retries (~5min while offline), so this is informational —
        // no manual retry button needed.
        return (
            <div className="updater-status updater-status--error" title={snapshot.error ?? undefined}>
                <Icon icon="warning-sign" size={14} />
                <span className="updater-status__label">Update check failed — will retry</span>
            </div>
        );
    }

    if (snapshot.phase === "ready") {
        // Bytes are installed to disk — only a relaunch remains. Persistent (one
        // of the three post-dismiss update paths: Settings, Fix All, Restart) so
        // the user still has a way to finish even after closing the dialog. No
        // re-check/re-download — just relaunch into the already-installed build.
        return (
            <button type="button" className="fix-all-btn" onClick={() => { void relaunch(); }}>
                <span className="fix-all-icon fix-all-icon--svg">
                    <Icon icon="refresh" size={20} />
                </span>
                <div className="fix-all-body">
                    <div className="fix-all-title">Restart WinCommander</div>
                    <div className="fix-all-sub">
                        {snapshot.version
                            ? `v${snapshot.version} installed — restart to finish`
                            : "Update installed — restart to finish"}
                    </div>
                </div>
            </button>
        );
    }

    // "idle" and "staged" render nothing here (staged → combined UpdateFlowDialog).
    if (snapshot.phase !== "available") {
        return null;
    }

    if (installing) {
        return (
            <button type="button" className="fix-all-btn" disabled aria-busy="true">
                <span className="fix-all-icon fix-all-icon--svg">
                    <Spinner size={20} />
                </span>
                <div className="fix-all-body">
                    <div className="fix-all-title">
                        Installing{snapshot.version ? ` v${snapshot.version}` : ""}…
                    </div>
                    <div className="fix-all-sub">Downloading — relaunching after install</div>
                </div>
            </button>
        );
    }

    return (
        <button type="button" className="fix-all-btn" onClick={handleManualInstall} disabled={installing}>
            <span className="fix-all-icon fix-all-icon--svg">
                <Icon icon="cloud-download" size={20} />
            </span>
            <div className="fix-all-body">
                <div className="fix-all-title">Update WinCommander</div>
                <div className="fix-all-sub">
                    {snapshot.version
                        ? `v${snapshot.version} available — click to install`
                        : "Update available — click to install"}
                </div>
            </div>
        </button>
    );
}
