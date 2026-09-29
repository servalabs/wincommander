// src/hooks/useProInstall.ts
//
// ═══════════════════════════════════════════════════════════════════════
// PRO INSTALL — Manifest fetch + install-action wrapper
// ═══════════════════════════════════════════════════════════════════════
//
// After a Free user activates their licence key (see LicenseGate), they
// still need the Pro sidecar binary on disk before any paid feature can
// run. The Tauri command install_pro_binary already handles the heavy
// lifting -- Defender exclusion, signed-URL fetch with 5-min timeout,
// SHA-256 verify, fsync, atomic rename. This hook is the React-side
// caller it was missing.
//
// Flow:
//   1. On first mount, fetch /pro/latest.json from the Pro artifact
//      origin. Manifest shape matches
//      what tools/release.ps1's Pro variant publishes:
//        { version, pub_date, notes, url, sha256, size }
//   2. Also call get_pro_install_status to know if Pro is already on disk.
//   3. install(consent) invokes install_pro_binary with the manifest's
//      url + sha256 + the explicit consent flag for the Defender
//      exclusion (required by the Rust-side guard).
//
// State is module-scoped via useSyncExternalStore so multiple consumers
// (LicenseGate auto-trigger, LicenseQuickPanel sidebar button, Identity
// panel banner) share one install lifecycle without re-fetching.

import { useCallback, useEffect, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { useAppState } from "../context/AppContext";
import { proReleaseCompatibilityError } from "../lib/proReleaseCompatibility";

// Base URL for Pro manifests. The per-version path ensures each Free release
// only downloads the Pro binary that was tested alongside it, preventing
// hash mismatches when Pro is released separately. Falls back to /pro/latest.json
// if the version-specific path 404s (not_published), so forward-compatibility
// is maintained. Must stay in lock-step with pro_install.rs's
// ALLOWED_UPDATE_HOST.
const PRO_MANIFEST_BASE = "https://winupdates.servalabs.com";

// Cached Free version (resolved once per session).
let cachedFreeVersion: string | null = null;

async function getFreeVersion(): Promise<string | null> {
    if (cachedFreeVersion !== null) return cachedFreeVersion;
    try {
        cachedFreeVersion = await getVersion();
        return cachedFreeVersion;
    } catch {
        return null;
    }
}

/** Sync access to the cached Free version string. Returns null until the
 *  first async call to getFreeVersion() (triggered by useProInstall mount)
 *  has resolved. Use only after the hook has had a chance to populate the cache. */
export function getCachedFreeVersion(): string | null {
    return cachedFreeVersion;
}

/** Download compatibility is a range; the authenticated native Hello remains the runtime gate. */
export function isProVersionCompatible(
    proVersion: string | null | undefined,
    freeVersion: string | null | undefined,
): boolean {
    return proReleaseCompatibilityError(proVersion, freeVersion) === null;
}

async function resolveManifestUrl(): Promise<string> {
    const freeVersion = await getFreeVersion();
    if (freeVersion) {
        // Attempt version-specific path first. If 404, fall back to latest.
        return `${PRO_MANIFEST_BASE}/pro/v${freeVersion}/latest.json`;
    }
    return `${PRO_MANIFEST_BASE}/pro/latest.json`;
}

// Resolved once on first use and cached for the session.
let resolvedManifestUrl: string | null = null;

export interface ProManifest {
    version: string;
    pub_date?: string;
    notes?: string;
    url: string;
    sha256: string;
    size?: number;
}

export interface ProInstallStatus {
    installed: boolean;
    install_path?: string | null;
    dev_path?: string | null;
    resolved_path?: string | null;
    /** SHA-256 (lowercase hex) of the local Pro EXE; null if not installed.
     *  Compared against the manifest's `sha256` to detect a stale binary. */
    local_sha256?: string | null;
    /** Version recorded when Pro was installed. Older installs may not have metadata. */
    local_version?: string | null;
}

export interface DefenderStatus {
    /** "on" | "off" | "unknown" */
    tamper_protection: string;
    /** "on" | "off" | "unknown" */
    real_time_monitoring: string;
    /** True when the Pro install dir is already in Defender's exclusion list. */
    exclusion_already_set: boolean;
}

/** Stages match the prefixes Rust prepends in install_pro_binary's Err strings. */
export type InstallStage =
    | "entitlement"
    | "elevation"
    | "consent"
    | "validation"
    | "defender_exclusion"
    | "download"
    | "sha256_mismatch"
    | "disk"
    | "unknown";

export type InstallState =
    | { kind: "idle" }
    | { kind: "installing" }
    | { kind: "installed"; version: string }
    | { kind: "error"; stage: InstallStage; message: string };

const KNOWN_STAGES: InstallStage[] = [
    "entitlement",
    "elevation",
    "consent",
    "validation",
    "defender_exclusion",
    "download",
    "sha256_mismatch",
    "disk",
];

/** Parse a "stage:message" prefix written by Rust's install_pro_binary. */
function parseStagedError(raw: string): { stage: InstallStage; message: string } {
    const idx = raw.indexOf(":");
    if (idx > 0) {
        const tag = raw.slice(0, idx) as InstallStage;
        if (KNOWN_STAGES.includes(tag)) {
            return { stage: tag, message: raw.slice(idx + 1).trim() };
        }
    }
    return { stage: "unknown", message: raw };
}

interface SnapShot {
    manifest: ProManifest | null;
    status: ProInstallStatus | null;
    defender: DefenderStatus | null;
    install: InstallState;
    manifestError: string | null;
}

let state: SnapShot = {
    manifest: null,
    status: null,
    defender: null,
    install: { kind: "idle" },
    manifestError: null,
};

const subscribers = new Set<() => void>();
let manifestFetchInFlight = false;
let statusFetchInFlight = false;
let defenderFetchInFlight = false;

function setState(partial: Partial<SnapShot>) {
    state = { ...state, ...partial };
    subscribers.forEach((s) => s());
}

function subscribe(cb: () => void) {
    subscribers.add(cb);
    return () => {
        subscribers.delete(cb);
    };
}

// Manifest fetch goes through a Rust-side Tauri command rather than the
// webview's native fetch(). Reason: Cloudflare R2 (which serves
// /pro/latest.json) doesn't send Access-Control-Allow-Origin, so the
// webview's CORS check killed the request with a generic "Failed to
// fetch" in production builds. Rust's reqwest isn't subject to CORS,
// and reqwest is already a dep for the EXE download below. Two retries
// with a short back-off still catches transient connectivity hiccups;
// the Rust side stage-prefixes errors ("not_published:", "http_error:",
// "network:", "parse:") so the dialog can render the right copy.
const MANIFEST_FETCH_RETRIES = 2;
const MANIFEST_RETRY_BACKOFF_MS = 1_500;

async function fetchManifestOnce(): Promise<ProManifest> {
    // Resolve and cache the versioned URL. On first call we also try the
    // version-specific path; if it 404s we fall back to /pro/latest.json.
    if (!resolvedManifestUrl) {
        resolvedManifestUrl = await resolveManifestUrl();
    }

    async function tryFetch(url: string): Promise<ProManifest> {
        const raw = await invoke<unknown>("fetch_pro_manifest", { manifestUrl: url });
        const body = raw as ProManifest;
        if (!body || !body.url || !body.sha256 || !body.version) {
            throw new Error("Pro manifest is missing required fields (version / url / sha256)");
        }
        const compatibilityError = proReleaseCompatibilityError(body.version, await getFreeVersion());
        if (compatibilityError) {
            throw Object.assign(new Error(compatibilityError), { stage: "validation" });
        }
        return body;
    }

    try {
        return await tryFetch(resolvedManifestUrl);
    } catch (e) {
        const message = e instanceof Error ? e.message : String(e);
        // Version-specific manifest not yet published — fall back to latest.
        if (message.startsWith("not_published:") && resolvedManifestUrl !== `${PRO_MANIFEST_BASE}/pro/latest.json`) {
            const fallback = `${PRO_MANIFEST_BASE}/pro/latest.json`;
            resolvedManifestUrl = fallback;
            try {
                const body = await tryFetch(fallback);
                return body;
            } catch (e2) {
                const m2 = e2 instanceof Error ? e2.message : String(e2);
                if (m2.startsWith("not_published:")) {
                    const err = new Error(
                        "A compatible Pro download has not been published yet. Use the matching WinCommander + Pro setup. Nothing was installed."
                    );
                    (err as Error & { stage?: string }).stage = "not_published";
                    throw err;
                }
                throw manifestFetchError(e2);
            }
        }
        if (message.startsWith("not_published:")) {
            const err = new Error(
                "A compatible Pro download has not been published yet. Use the matching WinCommander + Pro setup. Nothing was installed."
            );
            (err as Error & { stage?: string }).stage = "not_published";
            throw err;
        }
        throw manifestFetchError(e);
    }
}

function manifestFetchError(error: unknown): Error & { stage?: string } {
    if (error instanceof Error) return error;
    const raw = String(error);
    const parsed = parseStagedError(raw);
    return Object.assign(new Error(parsed.message), { stage: parsed.stage });
}

/** Forces the next manifest fetch to re-resolve the version-pinned manifest
 *  URL against `freeVersion` instead of reusing the cached one from mount.
 *  getVersion() (and the URL/version caches above) keep reporting the OLD
 *  Free build until relaunch, so when useUpdateFlow's runFreeStep() installs
 *  a new Free version mid-flow, the Pro compat check that runs immediately
 *  after would otherwise still compare against the stale /pro/v<old>/latest.json
 *  manifest and could wrongly conclude "Pro already current" for a click that
 *  just changed which Free version Pro needs to match. */
function invalidateManifestCache(freeVersion: string | null): void {
    cachedFreeVersion = freeVersion;
    resolvedManifestUrl = null;
}

async function fetchManifest(): Promise<void> {
    if (manifestFetchInFlight) return;
    manifestFetchInFlight = true;
    let lastErr: unknown = null;
    try {
        for (let attempt = 0; attempt <= MANIFEST_FETCH_RETRIES; attempt++) {
            try {
                const body = await fetchManifestOnce();
                setState({ manifest: body, manifestError: null });
                return;
            } catch (err) {
                lastErr = err;
                // 404 (release not published) shouldn't trigger retries --
                // the answer won't change in 1.5s.
                const stage = (err as Error & { stage?: string })?.stage;
                if (stage === "not_published" || stage === "validation") break;
                if (attempt < MANIFEST_FETCH_RETRIES) {
                    await new Promise((r) => setTimeout(r, MANIFEST_RETRY_BACKOFF_MS));
                }
            }
        }
        const message = lastErr instanceof Error ? lastErr.message : String(lastErr);
        setState({ manifest: null, manifestError: message });
    } finally {
        manifestFetchInFlight = false;
    }
}

async function refreshStatus(): Promise<void> {
    if (statusFetchInFlight) return;
    statusFetchInFlight = true;
    try {
        const status = await invoke<ProInstallStatus>("get_pro_install_status");
        setState({ status });
    } catch {
        // Leave status null; the dialog handles "status unknown" gracefully.
    } finally {
        statusFetchInFlight = false;
    }
}

async function refreshDefender(): Promise<void> {
    if (defenderFetchInFlight) return;
    defenderFetchInFlight = true;
    try {
        const defender = await invoke<DefenderStatus>("get_defender_status");
        setState({ defender });
    } catch {
        // Treat probe failure as "unknown". The dialog still renders the
        // consent flow; Add-MpPreference will fail downstream if Tamper
        // Protection turns out to be on.
        setState({
            defender: {
                tamper_protection: "unknown",
                real_time_monitoring: "unknown",
                exclusion_already_set: false,
            },
        });
    } finally {
        defenderFetchInFlight = false;
    }
}

async function installPro(consentDefenderExclusion: boolean): Promise<void> {
    const m = state.manifest;
    if (!m) {
        setState({
            install: {
                kind: "error",
                stage: "validation",
                message:
                    state.manifestError ?? "A compatible Pro download is unavailable. Refresh License / Pro or use the matching WinCommander + Pro setup. Nothing was installed.",
            },
        });
        return;
    }
    const compatibilityError = proReleaseCompatibilityError(m.version, await getFreeVersion());
    if (compatibilityError) {
        setState({ install: { kind: "error", stage: "validation", message: compatibilityError } });
        return;
    }
    setState({ install: { kind: "installing" } });
    try {
        await invoke("install_pro_binary", {
            downloadUrl: m.url,
            expectedSha256: m.sha256,
            consentDefenderExclusion,
            proVersion: m.version,
        });
        const status = await invoke<ProInstallStatus>("get_pro_install_status");
        setState({ status });
        if (!status.installed || status.local_sha256?.toLowerCase() !== m.sha256.toLowerCase()
            || !isProVersionCompatible(status.local_version, getCachedFreeVersion())) {
            throw new Error("validation:Pro installation could not be verified. Refresh License / Pro before retrying; installation has not been confirmed.");
        }
        await refreshDefender();
        setState({ install: { kind: "installed", version: m.version } });
    } catch (err) {
        const raw = err instanceof Error ? err.message : String(err);
        const { stage, message } = parseStagedError(raw);
        setState({ install: { kind: "error", stage, message } });
    }
}

export function proInstallStateForNewAttempt(install: InstallState): InstallState {
    return install.kind === "installing" || install.kind === "idle" ? install : { kind: "idle" };
}

function resetInstall() {
    const install = proInstallStateForNewAttempt(state.install);
    if (install !== state.install) setState({ install });
}

export interface ProInstallProbePolicy {
    /** Read the local sidecar state. This may hash an installed Pro binary. */
    status: boolean;
    /** Fetch the version-pinned Pro manifest. */
    manifest: boolean;
    /** Inspect Defender only when an install/update surface needs it. */
    defender: boolean;
}

export default function useProInstall(policy: ProInstallProbePolicy) {
    const snap = useSyncExternalStore(subscribe, () => state, () => state);
    const { runStartupJob } = useAppState();

    useEffect(() => {
        if (policy.manifest && !snap.manifest && !manifestFetchInFlight && !snap.manifestError) {
            void runStartupJob({
                id: "pro-manifest",
                priority: "background",
                cost: "light",
                timeoutMs: 15_000,
                run: async (signal) => {
                    if (!signal.aborted) await fetchManifest();
                },
            });
        }
        if (policy.status && !snap.status && !statusFetchInFlight) {
            void runStartupJob({
                id: "pro-install-status",
                priority: "background",
                cost: "expensive",
                timeoutMs: 30_000,
                run: async (signal) => {
                    if (!signal.aborted) await refreshStatus();
                },
            });
        }
        if (policy.defender && !snap.defender && !defenderFetchInFlight) {
            void runStartupJob({
                id: "defender-status",
                priority: "background",
                cost: "expensive",
                timeoutMs: 30_000,
                run: async (signal) => {
                    if (!signal.aborted) await refreshDefender();
                },
            });
        }
    }, [policy.manifest, policy.status, policy.defender, runStartupJob, snap.manifest, snap.status, snap.defender, snap.manifestError]);

    const install = useCallback((consent: boolean) => installPro(consent), []);
    const reset = useCallback(() => resetInstall(), []);
    const refresh = useCallback(async () => {
        await Promise.all([fetchManifest(), refreshStatus(), refreshDefender()]);
    }, []);
    // Called by useUpdateFlow right after runFreeStep() installs a new Free
    // version within the same flow -- see invalidateManifestCache above for why
    // the passive per-session cache can't self-correct until relaunch.
    const refreshForFreeVersion = useCallback(async (freeVersion: string | null) => {
        invalidateManifestCache(freeVersion);
        await fetchManifest();
    }, []);

    return {
        manifest: snap.manifest,
        manifestError: snap.manifestError,
        status: snap.status,
        defender: snap.defender,
        isInstalled: snap.status?.installed ?? false,
        installState: snap.install,
        install,
        reset,
        refresh,
        refreshForFreeVersion,
    };
}
