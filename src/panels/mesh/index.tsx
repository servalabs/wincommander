
import {
    Button,
    Dialog,
    DialogBody,
    DialogFooter,
    NonIdealState,
    Spinner,
    ProgressBar,
    Tag,
    Icon,
    Tooltip,
    HTMLSelect
} from "@/components/ui/bp";
import { useCallback, useEffect, useState, useMemo, useRef } from "react";
import { motion } from "framer-motion";
import { panelVariants, panelTransition, DURATION_S, EASE } from "../../components/shared/motion";
import useBackend, { MeshVPNStatus, MeshVPNPeer, executeBackendCommand } from "../../hooks/useBackend";

// Backend errors from the underlying mesh engine binary include its
// product name verbatim ("tailscale", "tailscaled.exe", etc). Strip
// those out before surfacing to the user so the UI stays
// implementation-neutral. Anything not matched falls through unchanged.
function sanitizeMeshError(raw: string | null | undefined): string {
    if (!raw) return "";
    return raw
        .replace(/\btailscaled?\.exe\b/gi, "the mesh service")
        .replace(/\btailscaled\b/gi, "the mesh service")
        .replace(/\bTailscale\b/g, "Private Mesh")
        .replace(/\btailscale\b/g, "private mesh")
        // The "running as tailscaled.exe, pid NNNN" parenthetical leaks
        // process internals; drop it entirely once the binary name has
        // been replaced.
        .replace(/\s*\(which appears to be running as the mesh service,?\s*pid \d+\)/gi, "")
        .replace(/\s*\(which appears to be running as the mesh service\)/gi, "");
}
import { useAppState } from "../../context/AppContext";
import useEntitlements from "../../hooks/useEntitlements";
import { open } from "@tauri-apps/plugin-dialog";
import { showError } from "../../utils/toast";
import { runOperation } from "../../context/OperationContext";
import SectionCard from "../../components/shared/SectionCard";
import UniversalToggle from "../../components/shared/UniversalToggle";
import EmbeddedWebView from "../../components/shared/EmbeddedWebView";
import MeshGrid, { OsLogo } from "./MeshGrid";
import { isMeshPeerOnline, isMeshPeerStale } from "./meshPresence";
import VpnKillSwitchSection from "./VpnKillSwitchSection";
import HowItWorks from "./HowItWorks";
import PanelHeader from "../../components/shared/PanelHeader";
import './index.css';

const STALE_THRESHOLD_MS = 20 * 24 * 60 * 60 * 1000; // 20 days

// ── Tailscale login-page whitelabel ─────────────────────────────────
//
// The stock login.tailscale.com page renders inside our embedded webview
// with a big "tailscale" wordmark, "Log in to connect a device to your
// tailnet" copy, and footer links back to tailscale.com. We strip every
// Tailscale-brand reference so the user sees what feels like a generic
// Sign In form.
//
// Whitelabelled / self-hosted coordinators (Headscale, custom Tailscale
// tenants) get their OWN branding rendered — we must NOT erase it, so
// the helper below only injects when the auth URL host is *.tailscale.com.
//
// CSS alone can't change text content — it hides elements but can't
// rewrite "tailnet" → "mesh". The JS pass below walks text nodes and
// does the substitution; CSS handles visual chrome (logos, images,
// redundant footers).
const TAILSCALE_HIDE_CSS = [
    // Wordmark / header chrome — Tailscale switched from a .logo class
    // to an inline <img>/<svg> inside a flex container. Cover both old
    // and new markup so the strip survives their next re-skin.
    ".logo, [class*='logo']:not([class*='sso']):not([class*='provider']):not([class*='button']) { display: none !important; }",
    "header, [class*='header']:not([class*='heading']) { display: none !important; }",
    // Strip any image that mentions tailscale in src/alt (wordmark image)
    "img[src*='tailscale'], img[alt*='ailscale'], img[alt*='ailnet'] { display: none !important; }",
    // Hide standalone SVGs (the wordmark uses one), then RESTORE SVGs
    // inside SSO provider buttons so Google/Microsoft/GitHub/Apple icons
    // keep rendering on the auth buttons users click.
    "svg { display: none !important; }",
    "button svg, [role='button'] svg, .button-sso svg, .button-logo svg, [class*='sso'] svg, [class*='provider'] svg, [class*='auth'] svg, label svg { display: revert !important; }",
    // Footer: hide every link that points back to tailscale.com (Learn
    // more, Terms, Privacy, etc.) — by attribute so class-name churn
    // doesn't break the strip.
    "a[href*='tailscale.com'] { display: none !important; }",
    "footer, [class*='footer'], .legal, [class*='legal'] { display: none !important; }",
].join("\n");

/// JS that runs on every DOM mutation inside the embedded webview to
/// rewrite visible text. The Rust harness in server_apps.rs wraps this
/// in a debounced MutationObserver loop, so we keep the body small +
/// idempotent. Body only — Rust adds the (function(){…})() wrapper,
/// DOMContentLoaded gating, and the observer.
const TAILSCALE_WHITELABEL_JS = `
    var subs = [
        [/\\bTailscale\\b/g, 'Private Mesh'],
        [/\\btailscale\\b/g, 'private mesh'],
        [/\\btailnet\\b/gi, 'mesh'],
    ];
    var walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, null);
    var node;
    while ((node = walker.nextNode())) {
        var v = node.nodeValue;
        if (!v) continue;
        var changed = v;
        for (var i = 0; i < subs.length; i++) {
            changed = changed.replace(subs[i][0], subs[i][1]);
        }
        if (changed !== v) node.nodeValue = changed;
    }
    try {
        if (document.title && /tailscale|tailnet/i.test(document.title)) {
            document.title = 'Sign In — Private Mesh';
        }
    } catch (e) {}
`;

/// Return branding-strip artifacts only when the auth URL is on the
/// stock Tailscale coordinator. Whitelabelled hosts (anything that
/// isn't *.tailscale.com) keep their own page chrome.
function brandingFor(authUrl: string): { css?: string; js?: string } {
    try {
        const host = new URL(authUrl).hostname.toLowerCase();
        if (host === "tailscale.com" || host.endsWith(".tailscale.com")) {
            return { css: TAILSCALE_HIDE_CSS, js: TAILSCALE_WHITELABEL_JS };
        }
    } catch { /* fall through — leave branding alone on parse failure */ }
    return {};
}

// OsLogo is imported from MeshGrid and used in gateway chips below.

// KT: MODULE-level (not component state or ref). App.tsx keys the panel
// wrapper on `activePanel`, so PrivateMeshPanel is fully unmounted +
// remounted on every navigation away and back into Mesh — any useState/
// useRef here resets on each visit. The meshHowItWorksSeen write below is
// an async IPC round trip; if the user leaves and returns before it
// resolves, a component-local "already dismissed" flag would already be
// gone, and the freshly-mounted auto-open effect would read the still-
// stale (false) appSettings.app.meshHowItWorksSeen and show the explainer
// again. This flag is set synchronously the instant the write is kicked
// off and survives for the life of the renderer, so a fast revisit is
// covered regardless of the remount or the write still being in flight.
let hiwMarkedSeenThisSession = false;

// localStorage key for the first-visit explainer. Persists across relaunches
// within an install; the durable meshHowItWorksSeen setting covers app updates.
const HIW_SEEN_KEY = "mesh-how-it-works-seen";

function PrivateMeshPanel() {
    const { getMeshVPNStatus, setMeshVPNConfig, sendMeshVPNFile, startMeshVPNLogin, startMeshService, stopMeshService, connectMeshVPN, error: backendError } = useBackend();
    const { meshInstalled, meshStatus: cachedMeshStatus, refreshDependencies, refreshMesh, markMeshInstalled, dependencyStatus, appSettings, patchAppSettings } = useAppState();
    const { hasPaid } = useEntitlements();

    // Core data
    const [meshStatus, setMeshStatus] = useState<MeshVPNStatus | null>(cachedMeshStatus);
    const [isLoading, setIsLoading] = useState(!cachedMeshStatus);
    const [actionLoading, setActionLoading] = useState(false);
    const [applyError, setApplyError] = useState<string | null>(null);
    const [installLoading, setInstallLoading] = useState(false);
    const [installProgressText, setInstallProgressText] = useState<string | null>(null);
    // Service-control feedback for the DISCONNECTED hero action button.
    const [serviceActionLoading, setServiceActionLoading] = useState(false);
    // Boot-window guard for the full-screen "Connecting to mesh service…"
    // spinner. After this flag flips, the spinner is replaced with the
    // regular panel layout even if the backend probe hasn't completed
    // yet — the panel then renders DISCONNECTED + an action button so
    // the user is never stuck on an unactionable spinner. 3 s matches
    // the backend MESH_PROBE_TIMEOUT (5 s) with margin for IPC overhead.
    const [bootDelayElapsed, setBootDelayElapsed] = useState(false);
    useEffect(() => {
        const t = setTimeout(() => setBootDelayElapsed(true), 3000);
        return () => clearTimeout(t);
    }, []);

    // How it works explainer — auto-open on first visit, manual after that.
    // "Seen" persists in durable settings (installer-preserved) rather than the
    // WebView's localStorage, which could be dropped on update and re-trigger
    // the explainer after every upgrade.
    const [showHowItWorks, setShowHowItWorks] = useState(false);
    const hiwAutoOpenedRef = useRef(false);
    useEffect(() => {
        if (hiwAutoOpenedRef.current || !appSettings) return;
        hiwAutoOpenedRef.current = true;
        // Auto-open only on the genuine first visit. Three guards, any of which
        // being "seen" suppresses it: the session flag (same run), the durable
        // setting (survives app updates), and a localStorage flag (survives
        // relaunch reliably even if the durable write round-trip ever fails —
        // this is what stops the explainer re-popping on every relaunch).
        if (
            !hiwMarkedSeenThisSession &&
            appSettings.app.meshHowItWorksSeen !== true &&
            localStorage.getItem(HIW_SEEN_KEY) !== "1"
        ) {
            setShowHowItWorks(true);
        }
    }, [appSettings]);
    useEffect(() => {
        if (!showHowItWorks) return;
        // Mark seen the instant it opens — synchronously in localStorage (never
        // fails) and durably in settings (survives app updates).
        hiwMarkedSeenThisSession = true;
        try { localStorage.setItem(HIW_SEEN_KEY, "1"); } catch { /* private mode */ }
        if (appSettings && appSettings.app.meshHowItWorksSeen !== true) {
            patchAppSettings({ app: { meshHowItWorksSeen: true } }).catch((err) => {
                // Durable write didn't land — localStorage still covers relaunch,
                // but surface the failure so it's not silently lost.
                showError(err instanceof Error ? err.message : "Failed to save How It Works dismissal.");
            });
        }
    }, [showHowItWorks, appSettings, patchAppSettings]);

    // Staging state
    const [staging, setStaging] = useState(() => ({
        advertiseExitNode: !!(cachedMeshStatus as any)?.prefs?.AdvertiseExitNode,
        allowLanAccess: !!(cachedMeshStatus as any)?.prefs?.ExitNodeAllowLANAccess,
        unattended: !!(cachedMeshStatus as any)?.prefs?.Unattended,
        acceptRoutes: !!(cachedMeshStatus as any)?.prefs?.AcceptRoutes,
        acceptDNS: !!(cachedMeshStatus as any)?.prefs?.AcceptDNS,
        shieldsUp: !!(cachedMeshStatus as any)?.prefs?.ShieldsUp,
        exitNodeIP: (cachedMeshStatus as any)?.prefs?.ExitNodeIP || ""
    }));

    // Check if anything has changed compared to last fetched status
    const hasChanges = useMemo(() => {
        if (!meshStatus?.prefs) return false;
        const p = meshStatus.prefs;
        return (
            staging.advertiseExitNode !== !!p.AdvertiseExitNode ||
            staging.allowLanAccess !== !!p.ExitNodeAllowLANAccess ||
            staging.unattended !== !!p.Unattended ||
            staging.shieldsUp !== !!p.ShieldsUp ||
            staging.acceptRoutes !== !!p.AcceptRoutes ||
            staging.acceptDNS !== !!p.AcceptDNS ||
            staging.exitNodeIP !== (p.ExitNodeIP || "")
        );
    }, [staging, meshStatus]);

    // KT: Mirror hasChanges into a ref so refreshStatus doesn't need it in
    // its useCallback dep array. Without this, every toggle flip recreates
    // refreshStatus, which in turn re-fires the "mount-only" initial-load
    // effect with forceUpdateStaging=true — overwriting the user's staged
    // changes before they can click Apply (the primary apply-config breakage).
    const hasChangesRef = useRef(hasChanges);
    useEffect(() => { hasChangesRef.current = hasChanges; });

    useEffect(() => {
        if (meshStatus?.prefs && !hasChanges) {
            setStaging({
                advertiseExitNode: !!meshStatus.prefs.AdvertiseExitNode,
                allowLanAccess: !!meshStatus.prefs.ExitNodeAllowLANAccess,
                unattended: !!meshStatus.prefs.Unattended,
                acceptRoutes: !!meshStatus.prefs.AcceptRoutes,
                acceptDNS: !!meshStatus.prefs.AcceptDNS,
                shieldsUp: !!meshStatus.prefs.ShieldsUp,
                exitNodeIP: meshStatus.prefs.ExitNodeIP || ""
            });
        }
    }, [meshStatus, hasChanges]);

    // Dialog & File sharing
    const [sendFileTarget, setSendFileTarget] = useState<MeshVPNPeer | null>(null);
    const [isSendDialogOpen, setIsSendDialogOpen] = useState(false);
    const [selectedFile, setSelectedFile] = useState<string | null>(null);
    const [fileTransferLoading, setFileTransferLoading] = useState(false);
    const [fileTransferText, setFileTransferText] = useState<string | null>(null);

    const [loginInfo, setLoginInfo] = useState<{ url?: string; qrUrl?: string; output?: string; browserOpened?: boolean; message?: string; pid?: number } | null>(null);
    const [loginLoading, setLoginLoading] = useState(false);
    const [loginError, setLoginError] = useState<string | null>(null);
    // Polling state used by the Option-B browser-handoff flow. When the
    // backend opens the system browser instead of producing an embedded
    // auth URL, we sit on a "waiting for browser" screen and poll
    // mesh status every 2 s. As soon as the daemon reports it's logged
    // in, we close the screen.
    const [browserPollSeconds, setBrowserPollSeconds] = useState(0);

    // Seed staging from cached status
    useEffect(() => {
        if (cachedMeshStatus?.prefs && !meshStatus) {
            setMeshStatus(cachedMeshStatus);
            const prefs = (cachedMeshStatus as any).prefs;
            setStaging({
                advertiseExitNode: !!prefs.AdvertiseExitNode,
                allowLanAccess: !!prefs.ExitNodeAllowLANAccess,
                unattended: !!prefs.Unattended,
                acceptRoutes: !!prefs.AcceptRoutes,
                acceptDNS: !!prefs.AcceptDNS,
                shieldsUp: !!prefs.ShieldsUp,
                exitNodeIP: prefs.ExitNodeIP || ""
            });
            setIsLoading(false);
        }
    }, [cachedMeshStatus, meshStatus]);

    // Initial load and manual refresh
    const refreshStatus = useCallback(async (forceUpdateStaging = false, silent = false) => {
        // Never block the panel with a full-screen spinner on silent background refreshes
        if (!silent) setIsLoading(true);
        const res = await getMeshVPNStatus();
        if (res.success && res.data) {
            // Trust res.data.installed (fresh from this call) instead of
            // the cached meshInstalled, which can be stuck at false from
            // an earlier fetch that ran before Tailscale was reachable.
            // Otherwise the panel keeps showing "Not Installed" forever.
            setMeshStatus({
                ...res.data,
                installed: res.data.installed,
            });

            // Only update staging if forced (e.g. after APPLY) or if there are no pending changes.
            // KT: read via ref — not the closure — so this branch doesn't force a dep on hasChanges.
            if (res.data.prefs && (forceUpdateStaging || !hasChangesRef.current)) {
                setStaging({
                    advertiseExitNode: !!res.data.prefs.AdvertiseExitNode,
                    allowLanAccess: !!res.data.prefs.ExitNodeAllowLANAccess,
                    unattended: !!res.data.prefs.Unattended,
                    acceptRoutes: !!res.data.prefs.AcceptRoutes,
                    acceptDNS: !!res.data.prefs.AcceptDNS,
                    shieldsUp: !!res.data.prefs.ShieldsUp,
                    exitNodeIP: res.data.prefs.ExitNodeIP || ""
                });
            }
        }
        if (!silent) setIsLoading(false);
    }, [getMeshVPNStatus]); // KT: hasChanges removed — read via hasChangesRef to keep refreshStatus stable

    useEffect(() => {
        // Use silent ONLY when we have cached data (no spinner needed).
        // Without cache we go non-silent so setIsLoading(false) fires
        // when the probe completes — otherwise isLoading stays true
        // forever on first launch and the spinner branch below traps
        // the user on "Connecting to mesh service…" with no way out.
        //
        // KT: intentionally mount-only (empty deps). refreshStatus was
        // previously listed here, but refreshStatus changes whenever
        // hasChanges changes (every toggle flip), causing this effect
        // to re-fire with forceUpdateStaging=true and clobber the
        // user's staged values — that was the apply-config breakage.
        // cachedMeshStatus late-arrivals are handled by the seed effect.
        const silent = !!cachedMeshStatus;
        refreshStatus(true, silent);
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []); // mount-only — see KT note above

    // Background polling
    // KT: Mesh status does not change often — 30s is sufficient and avoids
    // spawning a new powershell.exe every 5s which was a major CPU source.
    useEffect(() => {
        const timer = setInterval(() => {
            refreshStatus(false, true);
        }, 30000);
        return () => clearInterval(timer);
    }, [refreshStatus]);

    // Browser-handoff sign-in polling. While we're sitting on the auth
    // screen (embedded webview or waiting screen), poll mesh status every
    // 2 s. We call the LOCAL refreshStatus — refreshMesh() only updates
    // the AppContext cache, not the panel's local meshStatus that drives
    // the isLoggedOut check.
    //
    // IMPORTANT: this and the two effects below MUST sit above the early
    // return at `if (!isInstalled)` further down — moving them past it
    // would change the hook count between renders and crash React with
    // "Rendered more hooks than during the previous render".
    useEffect(() => {
        if (!loginInfo?.browserOpened) return;
        const tick = setInterval(async () => {
            setBrowserPollSeconds((s) => s + 2);
            try {
                await refreshStatus(false, true);
            } catch { /* swallow — next tick retries */ }
        }, 2000);
        return () => clearInterval(tick);
    }, [loginInfo?.browserOpened, refreshStatus]);

    // Detect logged-in transition. Reads from local meshStatus (kept
    // fresh by the poll above), which is the same source isLoggedOut
    // uses — so the panel transitions in the same render cycle as the
    // state flip.
    useEffect(() => {
        if (!loginInfo?.browserOpened) return;
        const bs = meshStatus?.backendState?.toLowerCase();
        const online = meshStatus?.self?.Online === true;
        if (bs === 'running' && online) {
            setLoginInfo(null);
            setBrowserPollSeconds(0);
        }
    }, [loginInfo?.browserOpened, meshStatus]);

    // Hard cap: if the user abandoned the browser tab, fail open after
    // 10 minutes so the panel doesn't poll indefinitely.
    useEffect(() => {
        if (!loginInfo?.browserOpened) return;
        if (browserPollSeconds >= 600) {
            setLoginInfo(null);
            setBrowserPollSeconds(0);
            setLoginError('Sign-in timed out after 10 minutes. Click Sign In again when you are ready.');
        }
    }, [loginInfo?.browserOpened, browserPollSeconds]);

    // Handle committing staged changes
    const handleApplyChanges = async () => {
        setActionLoading(true);
        setApplyError(null);
        try {
            const res = await setMeshVPNConfig({
                AdvertiseExitNode: staging.advertiseExitNode,
                AllowLanAccess: staging.allowLanAccess,
                Unattended: staging.unattended,
                AcceptRoutes: staging.acceptRoutes,
                AcceptDNS: staging.acceptDNS,
                ExitNodeIP: staging.exitNodeIP,
                ShieldsUp: staging.shieldsUp,
                Force: true
            });
            if (res.success) {
                await refreshStatus(true); // force-sync staging to confirmed backend state
            } else {
                setApplyError(sanitizeMeshError(res.error) || "Failed to apply config.");
            }
        } catch (err) {
            setApplyError(err instanceof Error ? sanitizeMeshError(err.message) : "Failed to apply config.");
        } finally {
            setActionLoading(false);
        }
    };

    // Peer sorting logic
    const sortedPeers = useMemo(() => {
        const peers = [...(meshStatus?.peers || [])];
        const now = new Date();

        // Helper to get category score
        const getCategoryScore = (p: MeshVPNPeer) => {
            if (isMeshPeerOnline(p)) return 0; // Online
            if (!isMeshPeerStale(p, STALE_THRESHOLD_MS, now.getTime())) return 1; // Offline recently
            return 2; // Stale
        };

        // Helper to parse IP to numeric for sorting
        const ipToLong = (ip: string) => {
            const cleanIP = ip.split('/')[0];
            const parts = cleanIP.split('.');
            if (parts.length !== 4) return 0;
            return parts.reduce((acc, part) => (acc << 8) + parseInt(part, 10), 0) >>> 0;
        };

        return peers.sort((a, b) => {
            // Sort by category first
            const scoreA = getCategoryScore(a);
            const scoreB = getCategoryScore(b);
            if (scoreA !== scoreB) return scoreA - scoreB;

            // Then by IP
            const ipA = a.IPs?.[0] || "0.0.0.0";
            const ipB = b.IPs?.[0] || "0.0.0.0";
            return ipToLong(ipA) - ipToLong(ipB);
        });
    }, [meshStatus]);

    // File selection logic
    const handleFileSelect = async () => {
        try {
            const selected = await open({ multiple: false, title: "Select file to send" });
            if (selected && typeof selected === 'string') setSelectedFile(selected);
        } catch (e) {
            console.error("File selection failed", e);
        }
    };

    const handleSendFileCommit = async () => {
        if (!selectedFile || !sendFileTarget) return;
        setFileTransferLoading(true);
        setFileTransferText(`Sending to ${sendFileTarget.Hostname}…`);
        try {
            const res = await sendMeshVPNFile(selectedFile, sendFileTarget.Hostname);
            if (!res.success) throw new Error(res.error || "Mesh file transfer failed.");
            // The backend `tailscale file cp` blocks until the transfer completes,
            // so reaching here is a real confirmation — not just "queued". Show a
            // clear sent state and hold it long enough to read.
            setFileTransferText(`✓ Sent to ${sendFileTarget.Hostname} — check Downloads or the secure inbox on that device.`);
            setTimeout(() => {
                setIsSendDialogOpen(false);
                setSelectedFile(null);
                setSendFileTarget(null);
                setFileTransferText(null);
            }, 2400);
        } catch (err) {
            setFileTransferText(err instanceof Error ? err.message : String(err));
        } finally {
            setFileTransferLoading(false);
        }
    };

    const handleInstallMesh = async () => {
        setInstallLoading(true);
        setInstallProgressText("Preparing Private Mesh install...");
        try {
            await runOperation("Install Private Mesh VPN", [
                {
                    label: "Installing Private Mesh VPN",
                    fn: async () => {
                        setInstallProgressText("Installing mesh engine...");
                        const res = await executeBackendCommand("Install-Dependency", { Id: "meshVpn" });
                        if (!res.success) throw new Error(res.error || "Private Mesh install failed.");
                        if (res.data && (res.data as any).error) {
                            throw new Error((res.data as any).message || "Private Mesh install failed.");
                        }
                    },
                },
                {
                    label: "Refreshing mesh status",
                    fn: async () => {
                        setInstallProgressText("Refreshing mesh status...");
                        // Optimistically flip the cached flag so the UI's
                        // isInstalled check stops reporting "NOT DETECTED"
                        // the moment the installer reported success. The
                        // probe below may take a moment to see the freshly
                        // registered tailscale.exe; without this hint the
                        // UI flashed "NOT DETECTED" simultaneously with
                        // the "installed" task notification.
                        markMeshInstalled(true);
                        await Promise.allSettled([refreshDependencies(true), refreshMesh(true), refreshStatus(true, true)]);
                    },
                },
            ], { mode: "sequential", accent: "blue", failFast: true, autoDismissMs: 4000 });
            setInstallProgressText("Private Mesh installed. Sign in if prompted.");
            await refreshStatus(true);
        } catch (err) {
            setInstallProgressText(err instanceof Error ? err.message : String(err));
        } finally {
            setInstallLoading(false);
        }
    };

    // First-load spinner — bounded by `bootDelayElapsed` (3 s).
    //
    // We only block the panel with the full-screen "Connecting…" view
    // during the boot window. After 3 s we fall through to the regular
    // layout even if the probe is still in flight (or has failed) so
    // the user can see the DISCONNECTED hero + action button rather
    // than an unactionable spinner. This is the seatbelt that catches
    // the case where the daemon is wedged hard enough that even the
    // backend's 5 s timeout doesn't fire in time.
    if (isLoading && !meshStatus && !bootDelayElapsed) {
        return (
            <div className="panel-container">
                <div className="flex items-center gap-2 mb-6 text-[var(--color-text-muted)]">
                    <Spinner size={14} />
                    <span className="font-mono text-xs uppercase tracking-widest">Connecting to mesh service…</span>
                </div>
                {[1,2,3].map(i => (
                    <div key={i} className="bp5-skeleton rounded mb-3" style={{ height: 56 }} />
                ))}
            </div>
        );
    }

    // Note: previously a "Loading Config" spinner fired when meshStatus
    // had no prefs. This was a dead-end — `tailscale debug prefs` can
    // legitimately return empty when the daemon isn't fully signed in
    // yet, leaving the panel stuck on a permanent spinner. Now we let
    // the panel render with empty prefs (toggles default to off via
    // optional chaining); the NeedsLogin branch below handles auth.

    // ── Install detection ──────────────────────────────────────────────
    //
    // Trust whichever signal says installed. We consult FIVE sources so
    // that a single missing/failed probe doesn't hide the panel:
    //
    //   1. `meshInstalled` flag in AppContext (set by refreshMesh).
    //   2. meshStatus.installed (the canonical field on the probe payload).
    //   3. meshStatus.backendState — its presence implies a reachable daemon.
    //   4. meshStatus.MagicDNSSuffix / peers — same.
    //   5. **dependencyStatus** — the FREE-tier Get-DependencyStatus probe
    //      runs a plain Test-Path against tailscale.exe. Critical fallback
    //      for users who deactivated Pro: Get-MeshVPNStatus is paid, so
    //      refreshMesh fails silently and meshInstalled stays null — but
    //      Tailscale is clearly still on disk. Without this fifth signal
    //      the panel falsely shows "Private Mesh Not Detected" and asks
    //      the user to reinstall something already installed.
    const meshDep = dependencyStatus?.find(d => d.id === 'meshVpn' || d.panelId === 'private-mesh');
    const isInstalled =
        meshInstalled === true ||
        !!meshStatus?.installed ||
        !!meshStatus?.backendState ||
        !!meshStatus?.MagicDNSSuffix ||
        (meshStatus?.peers?.length ?? 0) > 0 ||
        !!meshDep?.installed;

    if (!isInstalled) {
        return (
            <>
                <HowItWorks open={showHowItWorks} onClose={() => setShowHowItWorks(false)} />
                <div className="p-12">
                    <NonIdealState
                        icon={<Icon icon="shield" size={64} color="var(--color-danger)" />}
                        title={<span className="text-2xl font-bold">Private Mesh Not Detected</span>}
                        description="The Private Mesh engine needs to be installed and running to use mesh features. Our network secures all node-to-node communication."
                        action={
                            <div className="flex gap-3 justify-center">
                                <Button
                                    intent="danger"
                                    large
                                    text={installLoading ? "INSTALLING PRIVATE MESH..." : "INSTALL PRIVATE MESH ENGINE"}
                                    icon={installLoading ? undefined : "download"}
                                    loading={installLoading}
                                    onClick={handleInstallMesh}
                                    className="px-8 mesh-install-btn"
                                />
                                <Button icon="help" text="HOW IT WORKS" large onClick={() => setShowHowItWorks(true)} className="px-6" />
                            </div>
                        }
                    />
                    {installProgressText && (
                        <div className={`mesh-install-progress ${installLoading ? "active" : ""}`}>
                            <div className="mesh-install-progress__bar" />
                            <span>{installProgressText}</span>
                        </div>
                    )}
                </div>
            </>
        );
    }

    // ── Paywall branch ────────────────────────────────────────────────
    //
    // Mesh IS installed (some signal said so above) but the user has no
    // paid entitlement. We can't probe Get-MeshVPNStatus because it's
    // gated, so we can't render the connected hero — but we also must
    // not push the user to reinstall something that's already on disk.
    //
    // Show a non-destructive paywall card so the user understands the
    // panel exists but is locked behind Pro / trial. The license sidebar
    // already exposes Get License + Have a Key buttons, so we don't
    // duplicate those here — we just explain what's locked.
    if (!hasPaid) {
        return (
            <>
                <HowItWorks open={showHowItWorks} onClose={() => setShowHowItWorks(false)} />
                <div className="p-12">
                    <NonIdealState
                        icon={<Icon icon="lock" size={64} color="var(--color-accent)" />}
                        title={<span className="text-2xl font-bold">Private Mesh — Pro Required</span>}
                        description={
                            <span>
                                Private Mesh is installed on this machine, but the runtime
                                control surface (status, sign-in, peers, file send) is a
                                paid feature. Activate a license — or start the 16-day
                                free trial — from the License panel in the bottom-left to
                                unlock it. The mesh engine continues running independently
                                in the background.
                            </span>
                        }
                        action={
                            <Button
                                icon="help"
                                text="HOW IT WORKS"
                                large
                                onClick={() => setShowHowItWorks(true)}
                                className="px-6"
                            />
                        }
                    />
                </div>
            </>
        );
    }

    const isLoggedOut = !!meshStatus?.loggedOut
        || meshStatus?.backendState?.toLowerCase() === "needslogin"
        || (meshStatus?.health || []).some(h => h.toLowerCase().includes("stopped"));

    const handleLogin = async () => {
        setLoginLoading(true);
        setLoginError(null);
        const res = await startMeshVPNLogin();
        if (res.success) {
            const data: any = res.data || null;
            if (data?.alreadyAuthenticated) {
                // Daemon silently re-established the session via cached
                // creds — no browser flow needed.
                await refreshMesh(true);
            } else if (data?.browserOpened) {
                // Backend spawned mesh CLI in a hidden console and
                // captured the AuthURL via daemon-state polling. Now
                // we render that URL in our embedded webview with the
                // brand-strip CSS — whitelabel as originally intended.
                // No system browser handoff (was leaking Tailscale UI).
                //
                // Existing `loginInfo.url` branch below renders the
                // EmbeddedWebView; we feed it the URL we just got.
                setBrowserPollSeconds(0);
                setLoginInfo(data);
            } else {
                setLoginInfo(data);
            }
        } else {
            setLoginError(res.error || "Login failed.");
        }
        setLoginLoading(false);
    };

    if (isLoggedOut) {
        // Phase 2 — auth URL obtained: load it in a native webview with branding stripped.
        // This is the whitelabel path: the user signs in INSIDE our app
        // with Tailscale chrome stripped via injected CSS/JS. The poll
        // effect below detects the daemon flip to Running+Online and
        // auto-closes the webview.
        if (loginInfo?.url) {
            return (
                <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
                    {/* Top bar — rendered ABOVE the native webview bounds so it is never covered */}
                    <div style={{
                        padding: "8px 12px",
                        display: "flex",
                        alignItems: "center",
                        gap: 8,
                        borderBottom: "1px solid var(--color-border)",
                        background: "var(--color-bg-primary)",
                        flexShrink: 0,
                    }}>
                        <Button
                            icon="arrow-left"
                            minimal
                            small
                            text="Cancel"
                            className="mesh-auth-cancel"
                            onClick={() => setLoginInfo(null)}
                        />
                        <span style={{
                            fontFamily: "var(--font-mono)",
                            fontSize: 10,
                            color: "var(--color-text-muted)",
                            letterSpacing: "0.12em",
                            textTransform: "uppercase",
                        }}>Private Mesh — Sign In</span>
                    </div>
                    {/* Native webview fills remaining space */}
                    {(() => {
                        const brand = brandingFor(loginInfo.url);
                        return (
                            <EmbeddedWebView
                                group="mesh-login"
                                id="tailscale-auth"
                                url={loginInfo.url}
                                customCss={brand.css}
                                customJs={brand.js}
                                ephemeral
                                label="Private Mesh Auth"
                                style={{ flex: 1, minHeight: 0 }}
                            />
                        );
                    })()}
                </div>
            );
        }

        // Phase 1 — no auth URL yet: prompt user to start login
        return (
            <div className="p-12" style={{ paddingTop: 80 }}>
                <NonIdealState
                    icon={<Icon icon="log-in" size={64} color="var(--color-warning)" />}
                    title={<span className="text-2xl font-bold">Private Mesh Logged Out</span>}
                    description="Private Mesh VPN is installed but logged out. Sign in to re-enable mesh features."
                    action={
                        <div className="logged-out-actions">
                            <Button
                                intent="warning"
                                icon="log-in"
                                text={loginLoading ? "STARTING LOGIN..." : "SIGN IN"}
                                loading={loginLoading}
                                onClick={handleLogin}
                                className="mesh-btn-premium login-btn"
                            />
                            {loginError && (
                                <p className="mt-4 p-2 bg-red-500/10 text-red-400 text-xs rounded border border-red-500/20">
                                    {loginError || "Unknown error"}
                                </p>
                            )}
                        </div>
                    }
                />
            </div>
        );
    }

    const isActiveTunnel = !!meshStatus?.prefs?.ExitNodeIP;

    // ── Connection-state summary for the hero card ─────────────────
    //
    // Boils the half-dozen possible backend states down to a single
    // {label, button?} the hero can render. Previously the hero
    // unconditionally showed "DISCONNECTED" whenever Self.Hostname was
    // empty, which lumped together service-stopped, daemon-starting,
    // and not-signed-in into one unactionable string.
    //
    //   - service stopped                  → Start Mesh Service button
    //   - probe timed out                  → Restart Mesh Service button
    //   - backendState NoState             → "Not connected" + Connect button
    //                                        (daemon running, no live link —
    //                                         `tailscale up` reconnects with
    //                                         stored auth, or surfaces an
    //                                         AuthURL we route into login)
    //   - backendState Starting            → "Starting…" (no button — auto-resolves)
    //   - backendState NeedsLogin          → handled elsewhere by the auth flow
    //   - backendState Stopped             → Start Mesh Service button
    //   - Running but no IP yet            → "Connecting…" (no button)
    //   - Running + IP                     → connected (no callout)
    //
    // Returning null means "render the normal connected hero (hostname)".
    const connectionCallout = (() => {
        if (!meshStatus) {
            return { label: "Mesh service unreachable", action: "restart" as const };
        }
        const err = (meshStatus as any).error as string | undefined;
        const errCode = (meshStatus as any).errorCode as string | undefined;
        if (errCode === "DAEMON_TIMEOUT") {
            return { label: "Mesh daemon not responding", action: "restart" as const };
        }
        const bs = meshStatus.backendState?.toLowerCase();
        if (bs === "stopped" || bs === undefined || bs === null || bs === "") {
            // backendState empty + service known stopped (probe early-out
            // returns backendState='Stopped' too — both end up here).
            return { label: "Mesh service is stopped", action: "start" as const };
        }
        if (bs === "nostate") {
            // Daemon running but no active connection. One-click recovery:
            // run `tailscale up` to reconnect with whatever auth is stored.
            return { label: "Not connected", action: "connect" as const };
        }
        if (bs === "starting") {
            return { label: "Starting…", action: "none" as const };
        }
        if (bs === "running" && !meshStatus.self?.Hostname) {
            return { label: "Connecting…", action: "none" as const };
        }
        if (err) {
            return { label: err, action: "restart" as const };
        }
        return null; // healthy
    })();

    const handleStartMeshService = async () => {
        setServiceActionLoading(true);
        try {
            await startMeshService();
            await refreshStatus(true);
        } finally {
            setServiceActionLoading(false);
        }
    };

    const handleRestartMeshService = async () => {
        setServiceActionLoading(true);
        try {
            await stopMeshService();
            // Brief gap so the SCM finishes transitioning Stopped before
            // we ask it to start again — Start-Service throws if called
            // on a service that's still in StopPending.
            await new Promise((r) => setTimeout(r, 800));
            await startMeshService();
            await refreshStatus(true);
        } finally {
            setServiceActionLoading(false);
        }
    };

    // One-click recovery from NoState. We just launch the Tailscale
    // system-tray GUI (`tailscale-ipn.exe`) and let it own the auth
    // flow + daemon lifecycle. Backend returns `{launched:true}` on
    // success. We kick off a few fast follow-up refreshes (3 s, 8 s,
    // 15 s) so the chip flips as soon as the daemon transitions to
    // Running, without waiting for the 30 s background poll.
    const handleConnectMesh = async () => {
        setServiceActionLoading(true);
        setLoginError(null);
        try {
            const res = await connectMeshVPN();
            if (res.success && res.data) {
                await refreshStatus(true);
                setTimeout(() => { refreshStatus(true).catch(() => {}); }, 3000);
                setTimeout(() => { refreshStatus(true).catch(() => {}); }, 8000);
                setTimeout(() => { refreshStatus(true).catch(() => {}); }, 15000);
            } else if (!res.success) {
                setLoginError(res.error || "Connect failed.");
            }
        } finally {
            setServiceActionLoading(false);
        }
    };

    return (
        <>
        <HowItWorks open={showHowItWorks} onClose={() => setShowHowItWorks(false)} />
        {/* Replace dead animate-fade-in CSS class (no @keyframes existed) with
            framer panelVariants (opacity + y rise). MotionConfig in App.tsx
            handles reduced-motion automatically — no JS branch needed. */}
        <motion.div
            className={`mesh-panel-v2 ${isActiveTunnel ? 'tunnel-active' : ''}`}
            variants={panelVariants}
            initial="initial"
            animate="animate"
            exit="exit"
            transition={panelTransition}
        >
            <PanelHeader
                panelId="private-mesh"
                title="Private Network"
                description="Link your devices into a private, encrypted mesh — share files and reach them from anywhere."
            />
            {/* The overview card and the configuration grid deliberately share
                the PanelHeader's left edge. The app shell owns that inset. */}
            <div className={`mesh-hero-card ${isActiveTunnel ? 'tunnel-active-glow' : ''}`}>
                <div className="mesh-hero-content">
                    <div className="mesh-hero-main">
                        <div className="mesh-identity-row">
                            <div className={`mesh-status-ring ${meshStatus?.running ? 'is-online' : ''}`}>
                                <div className={`ring-inner ${meshStatus?.running ? 'active' : 'inactive'}`}>
                                    <Icon icon={meshStatus?.running ? "globe" : "offline"} size={24} />
                                </div>
                            </div>
                            <div className="mesh-identity-copy">
                                <div className="mesh-identity-name-row">
                                    <h1 className="mesh-identity-name">
                                        {meshStatus?.self?.Hostname || (connectionCallout?.label ?? "DISCONNECTED")}
                                    </h1>
                                    {meshStatus?.prefs?.ExitNodeIP && (
                                        <Tag intent="warning" minimal className="bg-orange-500/10 text-orange-400 border border-orange-500/20 text-[8px] font-black uppercase tracking-widest px-1.5">
                                            Tunnel Active
                                        </Tag>
                                    )}
                                </div>
                                {(() => {
                                    const ip = meshStatus?.self?.IPs?.[0];
                                    return (
                                        <span
                                            className="mesh-ip-badge mesh-identity-ip"
                                            title={ip ? "Click to copy IP address" : undefined}
                                            onClick={ip ? () => { void navigator.clipboard?.writeText(ip).catch(() => {}); } : undefined}
                                        >
                                            <span className="mesh-identity-ip__label">IP</span>
                                            <span className="mesh-identity-ip__value">{ip || "0.0.0.0"}</span>
                                        </span>
                                    );
                                })()}
                                {/* Action button for service-recovery states.
                                    Only renders when the callout asks for one;
                                    `none` callouts (Starting / Connecting) just
                                    show the label and let the 30 s background
                                    poll resolve them silently. */}
                                {connectionCallout?.action === "start" && (
                                    <Button
                                        small
                                        intent="primary"
                                        icon="play"
                                        text="Start Mesh Service"
                                        loading={serviceActionLoading}
                                        onClick={handleStartMeshService}
                                        className="mesh-recovery-btn"
                                    />
                                )}
                                {connectionCallout?.action === "connect" && (
                                    <Button
                                        small
                                        intent="primary"
                                        icon="globe"
                                        text="Connect Mesh"
                                        loading={serviceActionLoading}
                                        onClick={handleConnectMesh}
                                        className="mesh-recovery-btn"
                                    />
                                )}
                                {connectionCallout?.action === "restart" && (
                                    <Button
                                        small
                                        intent="warning"
                                        icon="refresh"
                                        text="Restart Mesh Service"
                                        loading={serviceActionLoading}
                                        onClick={handleRestartMeshService}
                                        className="mesh-recovery-btn"
                                    />
                                )}
                            </div>
                        </div>
                        <VpnKillSwitchSection />
                    </div>

                    <div className="flex flex-col items-end gap-2">
                        <div className="flex gap-3">
                            {hasChanges && (
                                <Button
                                    intent="primary"
                                    icon="cloud-upload"
                                    text="APPLY CONFIG"
                                    onClick={handleApplyChanges}
                                    loading={actionLoading}
                                    disabled={isLoggedOut}
                                    className="mesh-btn-premium glow-blue"
                                />
                            )}
                            <Tooltip content="How Private Mesh works" position="top">
                                <Button
                                    icon="help"
                                    onClick={() => setShowHowItWorks(true)}
                                    aria-label="How Private Mesh works"
                                    className="mesh-btn-premium"
                                />
                            </Tooltip>
                            <Tooltip content="Refresh Private Mesh status" position="top">
                                <Button
                                    icon="refresh"
                                    onClick={() => refreshStatus(true)}
                                    loading={isLoading}
                                    aria-label="Refresh Private Mesh status"
                                    className="mesh-btn-premium"
                                />
                            </Tooltip>
                        </div>
                        {applyError && (
                            <p className="text-[11px] text-amber-400 bg-amber-500/10 rounded border border-amber-500/20 px-3 py-1.5 max-w-xs text-right leading-snug">
                                {applyError}
                            </p>
                        )}
                    </div>
                </div>
            </div>

            <div className="mesh-content-grid pb-6">
                {/* Error banner gating:
                    - `meshStatus.error` is the live-response error: trust it.
                    - `backendError` is `useBackend()`'s sticky error state — it
                      captures the LAST failure from any execute() call in this
                      hook instance and is only cleared at the START of the
                      next execute(). That means a transient first-probe
                      failure (e.g. license cache not yet loaded → Pro
                      entitlement error) lingers even after subsequent probes
                      succeed and populate meshStatus. Only fall back to
                      backendError when we have no live status at all,
                      otherwise the banner contradicts the connected hero
                      sitting right above it. */}
                {((!meshStatus && backendError) || meshStatus?.error) && (() => {
                    const rawErr = meshStatus?.error || backendError || "";
                    const cleanErr = sanitizeMeshError(rawErr);
                    // Detect the "different user owns the service" case
                    // (mesh engine returns "401 Unauthorized: running in
                    // server mode (PC\X); connection from PC\Y not
                    // allowed"). Show a more actionable hint for that
                    // specific case.
                    const userMismatch = /401 unauthorized.*server mode.*connection from/i.test(rawErr);
                    return (
                        // Replace dead animate-slide-up CSS class (no @keyframes existed).
                        // WHY: error banner should fade in from below on mount only —
                        // opacity + y transform stays on the compositor, no reflow.
                        <motion.div
                            className="mb-6"
                            initial={{ opacity: 0, y: 6 }}
                            animate={{ opacity: 1, y: 0 }}
                            transition={{ duration: DURATION_S.normal, ease: EASE.enter }}
                        >
                            <p role="alert" className="p-3 bg-amber-500/10 text-amber-400 text-xs rounded border border-amber-500/20">
                                <strong>Private Mesh service unreachable.</strong> {cleanErr}
                                <br />
                                <span className="text-amber-300/80">
                                    {userMismatch
                                        ? "The mesh service is registered to a different Windows user account. Sign out and back in as that user, or reinstall Private Mesh while signed in as the current user."
                                        : "The mesh control channel is admin-protected. Try: relaunch this app as Administrator, or restart the Private Mesh service from Windows Services."}
                                </span>
                            </p>
                        </motion.div>
                    );
                })()}
                {/* Configuration Section */}
                <div className="grid grid-cols-1 lg:grid-cols-12 gap-4 mb-8">
                    {/* Node Preferences Column */}
                    <div className="lg:col-span-7 flex flex-col">
                        <SectionCard title="Local Config" className="h-full">
                            <div className="grid grid-cols-[minmax(0,1.18fr)_minmax(0,1fr)] gap-x-4 gap-y-4">
                                <UniversalToggle
                                    label="Block Incoming"
                                    description="Drop unsolicited traffic"
                                    checked={staging.shieldsUp}
                                    onChange={(val) => setStaging(s => ({ ...s, shieldsUp: val }))}
                                    disabled={actionLoading || isLoggedOut}
                                />
                                <UniversalToggle
                                    label="Stay Connected"
                                    description="Run unattended"
                                    checked={staging.unattended}
                                    onChange={(val) => setStaging(s => ({ ...s, unattended: val }))}
                                    disabled={actionLoading || isLoggedOut}
                                />
                                <UniversalToggle
                                    label="Share Local Network"
                                    description="Accept subnet routes"
                                    checked={staging.acceptRoutes}
                                    onChange={(val) => setStaging(s => ({ ...s, acceptRoutes: val }))}
                                    disabled={actionLoading || isLoggedOut}
                                    className="mesh-toggle-share-lan"
                                />
                                <UniversalToggle
                                    label="Use Private DNS"
                                    description="Use MagicDNS"
                                    checked={staging.acceptDNS}
                                    onChange={(val) => setStaging(s => ({ ...s, acceptDNS: val }))}
                                    disabled={actionLoading || isLoggedOut}
                                />
                            </div>
                        </SectionCard>

                    </div>

                    {/* External Tunnel Column — match Local Config height while
                        letting the LAN / Share controls fill the card body. */}
                    <div className="lg:col-span-5 flex flex-col">
                        <SectionCard title="Tunnel Gateway" className="h-full">
                            <div className="mesh-gateway-shell">
                                <div className="mesh-gateway-layout">
                                    {/* Inline chips (like the reference HTML) instead of a
                                        dropdown: "Direct connection" + one chip per discovered
                                        gateway, each tagged with its OS icon (getOSIcon covers
                                        Windows / macOS / Linux / Android / iOS, with a generic
                                        device fallback). */}
                                    <div className="mesh-gateway-selector">
                                        <div className="mesh-gateway-chips" role="group" aria-label="Tunnel gateway">
                                            <button
                                                type="button"
                                                className={`mesh-gw-chip ${(!staging.exitNodeIP && !staging.advertiseExitNode) ? "on" : ""}`}
                                                onClick={() => setStaging(s => ({ ...s, exitNodeIP: "", advertiseExitNode: false }))}
                                                disabled={actionLoading || isLoggedOut}
                                                aria-pressed={!staging.exitNodeIP && !staging.advertiseExitNode}
                                                title="No external route — traffic leaves this device directly"
                                            >
                                                <Icon icon="globe" size={14} />
                                                Direct connection
                                            </button>
                                            {(() => {
                                                const exitNodes = meshStatus?.peers?.filter(p => !!p.ExitNodeOption) || [];
                                                if (exitNodes.length === 0) {
                                                    return <span className="mesh-gw-empty">No gateways discovered</span>;
                                                }
                                                return exitNodes.map(p => {
                                                    const cleanIp = p.IPs?.[0]?.split('/')[0] || "";
                                                    const active = staging.exitNodeIP === cleanIp;
                                                    return (
                                                        <button
                                                            key={p.ID}
                                                            type="button"
                                                            className={`mesh-gw-chip ${active ? "on" : ""}`}
                                                            onClick={() => setStaging(s => ({ ...s, exitNodeIP: cleanIp, advertiseExitNode: false }))}
                                                            disabled={actionLoading || isLoggedOut}
                                                            aria-pressed={active}
                                                            title={`Route via ${p.Hostname} (${p.OS || "unknown OS"}) · ${cleanIp}`}
                                                        >
                                                            <OsLogo os={p.OS} size={18} />
                                                            Via {p.Hostname}
                                                        </button>
                                                    );
                                                });
                                            })()}
                                        </div>
                                    </div>

                                    <div className="mesh-gateway-toggles">
                                        <button
                                            type="button"
                                            className={`mesh-mini-toggle ${staging.allowLanAccess ? "active" : ""}`}
                                            disabled={actionLoading || !staging.exitNodeIP || isLoggedOut}
                                            onClick={() => setStaging(s => ({ ...s, allowLanAccess: !s.allowLanAccess }))}
                                            aria-pressed={staging.allowLanAccess}
                                            aria-label={`Allow local network access: ${staging.allowLanAccess ? "on" : "off"}`}
                                        >
                                            <span className="mesh-mini-toggle-copy">
                                                <strong>LAN</strong>
                                                <small>Local network</small>
                                            </span>
                                            <span className="mesh-mini-switch" aria-hidden="true">
                                                <span />
                                            </span>
                                        </button>
                                        <button
                                            type="button"
                                            className={`mesh-mini-toggle ${staging.advertiseExitNode ? "active" : ""}`}
                                            disabled={actionLoading || isLoggedOut}
                                            onClick={() => setStaging(s => ({
                                                ...s,
                                                advertiseExitNode: !s.advertiseExitNode,
                                                exitNodeIP: !s.advertiseExitNode ? "" : s.exitNodeIP
                                            }))}
                                            aria-pressed={staging.advertiseExitNode}
                                            aria-label={`Share this device as a tunnel gateway: ${staging.advertiseExitNode ? "on" : "off"}`}
                                        >
                                            <span className="mesh-mini-toggle-copy">
                                                <strong>Share</strong>
                                                <small>Gateway</small>
                                            </span>
                                            <span className="mesh-mini-switch" aria-hidden="true">
                                                <span />
                                            </span>
                                        </button>
                                    </div>
                                </div>
                            </div>
                        </SectionCard>
                    </div>
                </div>

                {/* Network Visualization */}
                <div className="space-y-4">
                    <MeshGrid
                        title="Mesh Devices"
                        peers={sortedPeers}
                        onSendFile={(p) => { setSendFileTarget(p); setIsSendDialogOpen(true); }}
                        activeExitNodeIP={meshStatus?.activeExitNodeIP}
                        staleThresholdMs={STALE_THRESHOLD_MS}
                    />
                </div>
            </div>

            {/* Error Message */}


            {/* Mesh file-send dialog */}
            <Dialog
                isOpen={isSendDialogOpen}
                onClose={() => setIsSendDialogOpen(false)}
                title={<div className="text-xs font-mono font-black uppercase tracking-[0.2em]">Transmit Archive</div>}
                className="mesh-dialog"
                isCloseButtonShown={true}
            >
                <DialogBody>
                    <p className="mb-6 text-[13px] text-[var(--color-text-secondary)]">
                        Send a file securely to another device on the mesh. The transfer is encrypted end-to-end.
                    </p>
                    <div className="mb-4">
                        <div className="text-[10px] font-mono font-bold uppercase tracking-[0.18em] text-[var(--color-text-muted)] mb-2">
                            Recipient Device
                        </div>
                        <HTMLSelect
                            value={sendFileTarget?.ID || ""}
                            onChange={(e) => {
                                const peer = meshStatus?.peers?.find((p) => p.ID === e.currentTarget.value) || null;
                                setSendFileTarget(peer);
                                setFileTransferText(null);
                            }}
                            fill
                            options={[
                                { value: "", label: "— Select a device —" },
                                ...(meshStatus?.peers || [])
                                    .filter(isMeshPeerOnline)
                                    .map((peer) => ({
                                        value: peer.ID,
                                        label: peer.Hostname,
                                    }))
                            ]}
                        />
                    </div>
                    <button
                        type="button"
                        className="mesh-file-dropzone group"
                        onClick={handleFileSelect}
                        disabled={fileTransferLoading}
                        aria-label={selectedFile ? `Choose a different file. Selected ${selectedFile.split(/[\\/]/).pop()}` : "Choose a file to send"}
                    >
                        <Icon icon={selectedFile ? "document" : "upload"} size={32} className={selectedFile ? "text-[var(--color-accent)]" : "text-[var(--color-text-muted)]"} />
                        <div className="text-xs font-mono truncate w-full text-center mt-4">
                            {selectedFile ? (
                                <span className="text-[var(--color-text-primary)] font-bold">{selectedFile.split(/[\\/]/).pop()}</span>
                            ) : (
                                <span className="text-[var(--color-text-muted)] group-hover:text-[var(--color-text-primary)] transition-colors">Click to select file</span>
                            )}
                        </div>
                    </button>
                    <div className="mt-4 rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-card)] px-4 py-3">
                        <div className="text-[10px] font-mono font-bold uppercase tracking-[0.18em] text-[var(--color-text-muted)] mb-1">
                            Delivery
                        </div>
                        <div className="text-[12px] text-[var(--color-text-secondary)]">
                            {sendFileTarget
                                ? <>Recipient: <span className="text-[var(--color-accent)] font-bold">{sendFileTarget.Hostname}</span></>
                                : "Select a recipient device above."}
                        </div>
                        {sendFileTarget && (
                            <div className="text-[11px] text-[var(--color-text-muted)] mt-1">
                                The file will arrive in <strong>Downloads</strong> or the secure incoming folder on <strong>{sendFileTarget.Hostname}</strong>.
                            </div>
                        )}
                    </div>
                    {fileTransferText && (
                        <div className={`mesh-transfer-progress ${fileTransferLoading ? "active" : ""}`}>
                            {fileTransferLoading && <ProgressBar intent="primary" animate value={1} />}
                            <span>{fileTransferText}</span>
                        </div>
                    )}
                </DialogBody>
                <DialogFooter>
                    <Button text="ABORT" minimal onClick={() => setIsSendDialogOpen(false)} className="mesh-dialog-cancel" disabled={fileTransferLoading} />
                    <Button
                        intent="primary"
                        text="INITIATE TRANSFER"
                        onClick={handleSendFileCommit}
                        loading={fileTransferLoading}
                        disabled={!selectedFile || !sendFileTarget || fileTransferLoading}
                        className="mesh-btn-premium glow-blue min-w-[160px]"
                    />
                </DialogFooter>
            </Dialog>
        </motion.div>
        </>
    );
}

export default PrivateMeshPanel;
