// src/hooks/useUpdateFlow.ts
//
// Orchestrates the combined Free → Pro update flow for UpdateFlowDialog: one
// popup, sequential steps, no separate Pro popup.
//
// Sequence:
//   1. Free: reuse whatever the background scheduler already knows (updater
//      phase "staged" → install the already-downloaded bytes; anything else →
//      a fresh check, then install if available). Installing writes the new
//      binary to disk WITHOUT relaunching — the running process keeps
//      executing the old code in memory until relaunch() is explicitly
//      called, so it's safe to keep going.
//   2. If the signed licence does not cover paid updates, stop here.
//   3. Pro: reuse useProInstall's existing manifest/status/consent/install
//      state machine (same one InstallProDialog already drives) — this step
//      renders inline via renderProInstallStep, not a second dialog.
//      - "Pro ahead of Free": compatibility is re-checked against the version
//        runFreeStep just installed (NOT the live-reported one, which lags
//        until relaunch), so updating Free lets the Pro reinstall proceed in
//        this same modal. Only a Pro ahead of even the latest Free dead-ends
//        with a note (handle via the standalone controls once versions align).
//      - If Pro is already up to date, skip straight to done.
//   4. Done: one final screen. If Free was updated, offer a single restart
//      (covers whichever binaries changed — Pro never needs its own restart,
//      it's a separate sidecar process).
//
// Any failure (Free check/install, Pro manifest/install) surfaces inline with
// a retry, and the existing manual controls (Settings → Version Management,
// standalone InstallProDialog trigger) remain fully functional as a fallback
// regardless of where this flow stops.

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { useUpdater } from "./updaterStore";
import useProInstall, { isProVersionCompatible, getCachedFreeVersion } from "./useProInstall";

export type UpdateFlowPhase =
    | "idle"
    | "checking-free"
    | "installing-free"
    | "free-error"
    | "checking-pro"
    | "pro-step"
    | "done";

interface DohUpdateInfo {
    available: boolean;
    version?: string | null;
    current_version?: string | null;
    body?: string | null;
    date?: string | null;
}

/**
 * `automaticProInstallConsent` controls the Pro leg once it is needed:
 * - `null`: leave the Pro action to the visible flow.
 * - `true`: the visible flow has collected Defender consent.
 * - `false`: background updates may replace an already-approved Pro install,
 *   but Rust refuses to create a new Defender exclusion.
 */
export function useUpdateFlow(canUpdatePro: boolean, automaticProInstallConsent: boolean | null = null) {
    const updater = useUpdater();
    const pro = useProInstall({
        status: canUpdatePro,
        manifest: canUpdatePro,
        defender: canUpdatePro,
    });
    const { refreshForFreeVersion } = pro;
    const [phase, setPhase] = useState<UpdateFlowPhase>("idle");
    const [freeOutcome, setFreeOutcome] = useState<"updated" | "up-to-date" | null>(null);
    const [freeError, setFreeError] = useState<string | null>(null);
    const [proMismatch, setProMismatch] = useState(false);
    // Version runFreeStep just installed (or the live version when up-to-date).
    // The running process keeps reporting the OLD version until relaunch, so we
    // must use THIS for the Pro compatibility check, not getVersion().
    const [targetFreeVersion, setTargetFreeVersion] = useState<string | null>(null);
    const runningRef = useRef(false);

    const runFreeStep = useCallback(async (): Promise<{
        outcome: "updated" | "up-to-date";
        targetVersion: string | null;
    }> => {
        // "staged" means the background scheduler already downloaded and
        // verified an update — install the pre-downloaded bytes rather than
        // re-checking/re-downloading. Anything else (idle/available/no
        // scheduler activity yet) does a fresh on-demand check.
        // Already installed to disk (phase "ready") — don't re-install; report
        // it as updated so the flow proceeds to Pro / the restart step.
        if (updater.phase === "ready") {
            return { outcome: "updated", targetVersion: updater.version };
        }
        if (updater.phase === "staged") {
            await invoke("app_install_staged_update");
            return { outcome: "updated", targetVersion: updater.version };
        }
        const info = await invoke<DohUpdateInfo>("app_check_for_updates_doh");
        if (!info.available) {
            return { outcome: "up-to-date", targetVersion: info.current_version ?? getCachedFreeVersion() };
        }
        await invoke("app_install_update_doh");
        return { outcome: "updated", targetVersion: info.version ?? null };
    }, [updater.phase, updater.version]);

    const start = useCallback(async () => {
        if (runningRef.current) return;
        runningRef.current = true;
        setFreeError(null);
        setProMismatch(false);
        setPhase("checking-free");
        try {
            const { outcome, targetVersion } = await runFreeStep();
            setFreeOutcome(outcome);
            setTargetFreeVersion(targetVersion);
            if (outcome === "updated") {
                // Free just changed within this same flow -- force useProInstall's
                // manifest/URL cache to re-resolve against targetVersion before the
                // "checking-pro" effect below reads pro.manifest, or it can compare
                // against a manifest still resolved for the OLD Free version (see
                // invalidateManifestCache's doc comment in useProInstall.ts).
                await refreshForFreeVersion(targetVersion);
            }
            setPhase(canUpdatePro ? "checking-pro" : "done");
        } catch (err) {
            setFreeError(err instanceof Error ? err.message : String(err));
            setPhase("free-error");
        } finally {
            runningRef.current = false;
        }
    }, [runFreeStep, canUpdatePro, refreshForFreeVersion]);

    const retryFree = useCallback(() => { void start(); }, [start]);

    // Once we're waiting on Pro's manifest/status, decide where to go as
    // soon as they resolve (useProInstall fetches both automatically on
    // mount — this effect just reacts to that shared, module-scoped state).
    useEffect(() => {
        if (phase !== "checking-pro") return;
        if (pro.manifestError && !pro.manifest) {
            // Manifest fetch failed outright — the Pro step's own consent-state
            // body already renders manifestError, so route there.
            setPhase("pro-step");
            return;
        }
        if (!pro.manifest || !pro.status) return; // still loading
        // Version-timing wrinkle: getVersion() (and pro's live compat flag)
        // still report the OLD Free version until relaunch, so a Pro built for
        // the just-installed target Free would wrongly look "ahead". Re-check
        // against the version runFreeStep actually installed this run — that's
        // what lets a "Pro ahead of Free" case update Free and THEN reinstall
        // Pro in this same modal instead of dead-ending.
        const compatible = isProVersionCompatible(
            pro.manifest.version,
            targetFreeVersion ?? getCachedFreeVersion(),
        );
        if (!compatible) {
            // Pro is ahead of even the just-installed latest Free — genuinely
            // anomalous. Don't force it: surface the note and leave Pro to the
            // standalone controls once versions line up.
            setProMismatch(true);
            setPhase("done");
            return;
        }
        const hashOk = !!(
            pro.status.local_sha256 &&
            pro.manifest.sha256 &&
            pro.status.local_sha256.toLowerCase() === pro.manifest.sha256.toLowerCase()
        );
        if (pro.status.installed && hashOk) {
            setPhase("done"); // Pro already current — nothing to do
            return;
        }
        // Background automatic updates must never perform the first Pro
        // install: it requires the one-time Defender consent shown in the
        // visible install dialog. They may, however, replace an already
        // installed sidecar without altering Defender configuration.
        if (!pro.status.installed && automaticProInstallConsent === false) {
            setPhase("done");
            return;
        }
        setPhase("pro-step");
    }, [phase, pro.manifest, pro.manifestError, pro.status, targetFreeVersion, automaticProInstallConsent]);

    // The visible flow supplies true after consent; the background coordinator
    // supplies false and can therefore update only an existing approved install.
    const { install } = pro;
    const proInstallKind = pro.installState.kind;
    useEffect(() => {
        if (phase !== "pro-step") return;
        if (automaticProInstallConsent === null) return;
        if (proInstallKind !== "idle") return;
        void install(automaticProInstallConsent);
    }, [phase, automaticProInstallConsent, proInstallKind, install]);

    // Pro step reports success → the combined flow is done. (Rendering never
    // shows renderProInstallStep's own "installed" screen in the embedded
    // context — see UpdateFlowDialog — this transition is what takes over.)
    useEffect(() => {
        if (phase === "pro-step" && pro.installState.kind === "installed") {
            setPhase("done");
        }
    }, [phase, pro.installState.kind]);

    const finishAndRestart = useCallback(async () => {
        await relaunch();
    }, []);

    const reset = useCallback(() => {
        setPhase("idle");
        setFreeOutcome(null);
        setFreeError(null);
        setProMismatch(false);
        setTargetFreeVersion(null);
    }, []);

    return {
        phase,
        freeOutcome,
        freeError,
        proMismatch,
        pro,
        start,
        retryFree,
        finishAndRestart,
        reset,
        /** True once Free has actually been installed to disk this run — the
         *  running process needs a relaunch to pick it up. Pro never needs
         *  its own restart (separate sidecar process). */
        needsRestart: freeOutcome === "updated",
    };
}
