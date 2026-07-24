// src/components/UpdateFlowDialog.tsx
//
// ═══════════════════════════════════════════════════════════════════════
// UPDATE FLOW DIALOG — one popup, Free then Pro, in sequence
// ═══════════════════════════════════════════════════════════════════════
//
// Replaces the previous separate surfaces (RestartPrompt toast, Pro startup
// nag, ProUpdatePrompt card) with a single dialog: check/install Free first,
// then — only if the user has a paid entitlement — continue straight into
// checking/installing Pro in the SAME dialog (no second popup). One restart
// prompt at the end covers whichever binary actually changed.
//
// The Pro step reuses the exact same state machine and UI (consent /
// installing / error) as the standalone InstallProDialog via
// renderProInstallStep — nothing about the Defender-consent gate, version-
// compat guard, or error-stage handling is duplicated or re-implemented.
//
// If Pro's manifest is incompatible with the running Free version (Pro ahead
// of Free), this flow does NOT force it through — it stops with a note and
// leaves Pro to be handled separately via the existing standalone controls
// once versions line up.
//
// Triggered by: the app-wide auto-check (see App.tsx) and the "Check for
// Updates" action in Settings → Version Management — both drive this same
// dialog so the experience is identical either way.

import { useEffect, useState } from "react";
import { Button, Dialog, DialogBody, DialogFooter, Icon, Spinner } from "@/components/ui/bp";
import { useUpdateFlow } from "../hooks/useUpdateFlow";
import { renderProInstallStep } from "./shared/ProInstallStepBody";

interface UpdateFlowDialogProps {
    isOpen: boolean;
    onClose: () => void;
    hasPaid: boolean;
    /** True when the caller already knows an update/install is pending. Paid
     *  users then see an upfront confirm (covering the Free+Pro update AND the
     *  one-time Defender exclusion) before anything runs. Everyone else — and
     *  on-demand checks with nothing known pending — starts checking directly. */
    updateAvailable?: boolean;
    /** Consent-step "Not now" override — used only by the startup auto-trigger
     *  so App.tsx can persist the dismissed flag. On-demand (Settings button)
     *  invocations leave this undefined. */
    onNotNow?: () => void;
}

function CenteredStatus({ title, sub }: { title: string; sub?: string }) {
    return (
        <div style={{
            minHeight: 220,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            textAlign: "center",
            padding: "24px 0",
        }}>
            <Spinner size={48} />
            <h3 style={{ marginTop: 16, marginBottom: 8 }}>{title}</h3>
            {sub && <p style={{ color: "var(--color-text-muted)", fontSize: 12 }}>{sub}</p>}
        </div>
    );
}

export default function UpdateFlowDialog({ isOpen, onClose, hasPaid, updateAvailable = false, onNotNow }: UpdateFlowDialogProps) {
    // True once the user OKs the upfront confirm — collects the Defender-exclusion
    // consent for the whole Free+Pro update, so the Pro leg installs plainly
    // (no second consent gate) via useUpdateFlow's auto path.
    const [autoConsented, setAutoConsented] = useState(false);
    const flow = useUpdateFlow(hasPaid, autoConsented);
    const { phase, freeOutcome, freeError, proMismatch, pro, start, retryFree, finishAndRestart, reset, needsRestart } = flow;
    const [consent, setConsent] = useState(false);

    const showConfirm = hasPaid && updateAvailable && !autoConsented;

    // Kick off the sequence as soon as the dialog opens — unless a paid user
    // needs to OK the upfront confirm first. Reset when it closes so reopening
    // always starts a fresh check rather than showing stale state.
    useEffect(() => {
        if (isOpen && phase === "idle" && !showConfirm) void start();
        if (!isOpen && phase !== "idle") { reset(); setConsent(false); setAutoConsented(false); }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [isOpen]);

    const isBusy =
        phase === "checking-free" ||
        phase === "installing-free" ||
        phase === "checking-pro" ||
        (phase === "pro-step" && pro.installState.kind === "installing");

    let body: React.ReactNode;
    let footer: React.ReactNode;

    if (phase === "idle" && showConfirm) {
        // Upfront confirm for paid users: one consent covers the Free+Pro
        // update AND the one-time Defender exclusion, so no separate Pro
        // prompt appears later — the Pro leg installs plainly (autoConsented).
        body = (
            <div style={{ padding: "8px 0" }}>
                <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 12 }}>
                    <Icon icon="cloud-download" size={24} intent="primary" />
                    <h3 style={{ margin: 0 }}>Update WinCommander</h3>
                </div>
                <div
                    style={{
                        padding: 12,
                        border: "1px solid var(--color-warning)",
                        background: "var(--color-warning-dim)",
                        fontSize: 12,
                    }}
                >
                    Updating WinCommander will also update WinCommander Pro and add a
                    Windows Defender exclusion for{" "}
                    <code style={{ margin: "0 2px" }}>%ProgramData%\WinCommander\bin</code>.
                </div>
            </div>
        );
        footer = (
            <>
                <Button minimal className="wc-btn-ghost" onClick={onNotNow ?? onClose}>
                    Later
                </Button>
                <Button
                    className="wc-btn-primary"
                    icon="cloud-download"
                    onClick={() => { setAutoConsented(true); void start(); }}
                >
                    Update
                </Button>
            </>
        );
    } else if (phase === "idle") {
        // Renders for at most one frame between the dialog opening and the
        // effect above kicking off `start()` — avoids ever falling through
        // to the "done" branch below with stale/default values.
        body = <CenteredStatus title="Preparing…" />;
        footer = <Button minimal disabled>Please wait…</Button>;
    } else if (phase === "checking-free") {
        body = <CenteredStatus title="Checking for updates…" />;
        footer = <Button minimal disabled>Please wait…</Button>;
    } else if (phase === "installing-free") {
        body = <CenteredStatus title="Updating WinCommander…" sub="Downloading and verifying the new version." />;
        footer = <Button minimal disabled>Please wait…</Button>;
    } else if (phase === "free-error") {
        body = (
            <div style={{ padding: "8px 0" }}>
                <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 12 }}>
                    <Icon icon="error" size={24} intent="danger" />
                    <h3 style={{ margin: 0 }}>Couldn't check/update WinCommander</h3>
                </div>
                <p style={{ fontSize: 12, marginBottom: 12 }}>
                    You can still update manually anytime from Settings → Version Management.
                </p>
                <pre
                    style={{
                        color: "var(--color-text-muted)",
                        fontSize: 10,
                        fontFamily: "var(--font-mono)",
                        whiteSpace: "pre-wrap",
                        wordBreak: "break-word",
                        background: "var(--color-bg-tertiary)",
                        border: "1px solid var(--color-border)",
                        padding: 10,
                        margin: 0,
                    }}
                >
                    {freeError}
                </pre>
            </div>
        );
        footer = (
            <>
                <Button minimal onClick={retryFree}>Try again</Button>
                <Button className="wc-btn-ghost" onClick={onClose}>Close</Button>
            </>
        );
    } else if (phase === "checking-pro") {
        body = <CenteredStatus title="Checking WinCommander Pro…" />;
        footer = <Button minimal disabled>Please wait…</Button>;
    } else if (phase === "pro-step" && pro.installState.kind !== "installed") {
        // Reuses the exact InstallProDialog state machine/UI inline — same
        // consent gate, Defender pre-flight, and error-stage copy, just
        // embedded in this single dialog instead of a second popup.
        // Note: onClose here means "close the whole combined flow", not just
        // the Pro step — closing at any point leaves the standalone Settings
        // controls fully usable as a manual fallback.
        const step = renderProInstallStep({
            pro,
            consent,
            onConsentChange: setConsent,
            onNotNow,
            onClose,
            autoProgress: autoConsented,
        });
        body = step.body;
        footer = step.footer;
    } else if (phase === "pro-step") {
        // Transitional frame: install just succeeded; the flow effect below
        // is about to flip phase to "done" on the next render.
        body = <CenteredStatus title="Finishing up…" />;
        footer = <Button minimal disabled>Please wait…</Button>;
    } else {
        // "done"
        const freeLine =
            freeOutcome === "updated" ? "WinCommander updated." :
            freeOutcome === "up-to-date" ? "WinCommander is already up to date." :
            null;
        const proLine = !hasPaid
            ? null
            : proMismatch
                ? "A newer Pro release is available but needs a newer WinCommander version first — nothing to do right now."
                : pro.installState.kind === "installed"
                    ? `WinCommander Pro v${pro.installState.version} installed.`
                    : pro.status?.installed
                        ? "WinCommander Pro is already up to date."
                        : null;

        body = (
            <div style={{
                minHeight: 180,
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                justifyContent: "center",
                textAlign: "center",
                padding: "24px 0",
                gap: 6,
            }}>
                <Icon icon="tick-circle" size={40} intent="success" />
                <h3 style={{ marginTop: 12, marginBottom: 4 }}>You're all set</h3>
                {freeLine && <p style={{ color: "var(--color-text-muted)", fontSize: 13 }}>{freeLine}</p>}
                {proLine && <p style={{ color: "var(--color-text-muted)", fontSize: 13 }}>{proLine}</p>}
                {needsRestart && (
                    <p style={{ color: "var(--color-warning)", fontSize: 12, marginTop: 8 }}>
                        Restart to finish applying the WinCommander update.
                    </p>
                )}
            </div>
        );
        footer = needsRestart ? (
            <>
                <Button minimal className="wc-btn-ghost" onClick={onClose}>Later</Button>
                <Button
                    className="wc-btn-primary"
                    icon="refresh"
                    onClick={() => { void finishAndRestart(); }}
                >
                    Restart now
                </Button>
            </>
        ) : (
            <Button className="wc-btn-primary" onClick={onClose}>Done</Button>
        );
    }

    return (
        <Dialog
            isOpen={isOpen}
            onClose={isBusy ? undefined : onClose}
            title="Check for Updates"
            icon="cloud-download"
            canEscapeKeyClose={!isBusy}
            canOutsideClickClose={!isBusy}
            style={{ width: 520 }}
        >
            <DialogBody>{body}</DialogBody>
            <DialogFooter>{footer}</DialogFooter>
        </Dialog>
    );
}
