import { useEffect, useRef } from "react";
import { relaunch } from "@tauri-apps/plugin-process";
import { showError, showInfo } from "../utils/toast";
import { useUpdater } from "./updaterStore";
import useProInstall, { getCachedFreeVersion, isProVersionCompatible } from "./useProInstall";
import { useUpdateFlow } from "./useUpdateFlow";

export function proNeedsUpdate(
    localSha256: string | null | undefined,
    manifestSha256: string | null | undefined,
): boolean {
    // Pre-metadata installations have no local hash. They are still an
    // installed Pro copy, so treat them as needing the verified latest binary
    // instead of leaving them permanently outside the automatic update path.
    if (!manifestSha256) return false;
    if (!localSha256) return true;
    return localSha256.toLowerCase() !== manifestSha256.toLowerCase();
}

/**
 * Automatic updates may maintain an already installed Pro copy for a paid
 * user. They must not silently perform the first Pro install, because that
 * still needs the one-time Defender consent shown in the install dialog.
 */
export function canAutomaticallyUpdatePro(
    updateEntitled: boolean,
    installed: boolean | null | undefined,
): boolean {
    return updateEntitled && installed === true;
}

/** Runs a no-dialog update only after the user chose automatic updates. */
export default function useAutomaticUpdate(enabled: boolean, canUpdatePaidBuilds: boolean) {
    const updater = useUpdater();
    const pro = useProInstall({
        status: enabled && canUpdatePaidBuilds,
        manifest: enabled && canUpdatePaidBuilds,
        // Updating an existing Pro copy does not need a Defender probe or a
        // new exclusion. The Rust command preserves the first-install consent
        // boundary and only replaces an already installed sidecar here.
        defender: false,
    });
    const canUpdatePro = canAutomaticallyUpdatePro(canUpdatePaidBuilds, pro.status?.installed);
    // Keep the paid leg enabled even while the status probe is in flight. This
    // lets a Free update and an installed Pro update complete in one cycle;
    // useUpdateFlow skips only a missing first-time Pro installation.
    const flow = useUpdateFlow(canUpdatePaidBuilds, false);
    const { start, phase, freeError, needsRestart, pro: flowPro } = flow;
    const startedRef = useRef(false);
    const completedRef = useRef(false);

    useEffect(() => {
        if (!enabled || startedRef.current) return;

        const freePending = updater.phase === "available" || updater.phase === "staged";
        if (freePending) {
            startedRef.current = true;
            void start();
            return;
        }

        if (!canUpdatePro || !pro.status || !pro.manifest) return;
        if (!pro.status.installed) return;
        if (!proNeedsUpdate(pro.status.local_sha256, pro.manifest.sha256)) return;
        if (!isProVersionCompatible(pro.manifest.version, getCachedFreeVersion())) return;

        startedRef.current = true;
        void start();
    }, [enabled, canUpdatePro, updater.phase, pro.status, pro.manifest, start]);

    useEffect(() => {
        if (!enabled || !startedRef.current || completedRef.current) return;
        if (phase === "free-error" || flowPro.installState.kind === "error") {
            completedRef.current = true;
            const proError = flowPro.installState.kind === "error"
                ? flowPro.installState.message
                : "Automatic update failed.";
            showError(
                freeError ?? proError,
                undefined,
                { kind: "notification" },
            );
            return;
        }
        if (phase !== "done") return;
        completedRef.current = true;
        if (needsRestart) {
            showInfo("WinCommander was updated and will restart now.", undefined, { kind: "notification" });
            void relaunch().catch((error) => {
                showError(error instanceof Error ? error.message : String(error), undefined, { kind: "notification" });
            });
        }
    }, [enabled, freeError, needsRestart, phase, flowPro.installState]);
}
