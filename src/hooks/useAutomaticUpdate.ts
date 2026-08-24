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

/** Runs a no-dialog update only after the user chose automatic updates. */
export default function useAutomaticUpdate(enabled: boolean, hasPaid: boolean) {
    const updater = useUpdater();
    const pro = useProInstall({
        status: enabled && hasPaid,
        manifest: enabled && hasPaid,
        defender: enabled && hasPaid,
    });
    // Automatic mode may update Pro only after the person already approved its
    // Defender exclusion. It must never create that exclusion by itself.
    const canUpdatePro = hasPaid && pro.defender?.exclusion_already_set === true;
    const flow = useUpdateFlow(canUpdatePro, false);
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
