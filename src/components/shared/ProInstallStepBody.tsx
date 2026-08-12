// src/components/shared/ProInstallStepBody.tsx
//
// The consent / installing / installed / error content for the Pro sidecar
// install step, extracted out of InstallProDialog so it can be embedded
// inline inside UpdateFlowDialog's single combined popup (no second Dialog
// chrome) while InstallProDialog keeps rendering the exact same tested body
// wrapped in its own <Dialog> for standalone callers (LicenseGate,
// LicenseQuickPanel, the Settings "pro version mismatch → handle alone"
// fallback). Behavior, copy, and error-stage handling are unchanged from the
// original InstallProDialog — this is a pure extraction, not a rewrite.

import { Button, Checkbox, Icon, Spinner } from "@/components/ui/bp";
import type useProInstall from "../../hooks/useProInstall";

interface ProInstallStepBodyProps {
    pro: ReturnType<typeof useProInstall>;
    consent: boolean;
    onConsentChange: (v: boolean) => void;
    /** Called instead of the default close when the user clicks "Not now"
     *  from the startup-nag path. Undefined ⇒ falls back to onClose. */
    onNotNow?: () => void;
    onClose: () => void;
    /** Combined UpdateFlowDialog auto path: the Defender-exclusion consent was
     *  already captured by that dialog's upfront confirm and the install fires
     *  automatically (see useUpdateFlow), so the idle state renders plain
     *  progress instead of the interactive consent gate. Standalone callers
     *  (InstallProDialog) omit it and keep the consent flow unchanged. */
    autoProgress?: boolean;
}

export interface ProInstallStepRender {
    body: React.ReactNode;
    footer: React.ReactNode;
    isBusy: boolean;
}

export function renderProInstallStep({
    pro,
    consent,
    onConsentChange,
    onNotNow,
    onClose,
    autoProgress = false,
}: ProInstallStepBodyProps): ProInstallStepRender {
    const { manifest, manifestError, defender, installState, install, reset, refresh } = pro;

    const tamperOn = defender?.tamper_protection === "on";
    const exclusionAlreadySet = defender?.exclusion_already_set === true;
    const isBusy = installState.kind === "installing";

    let body: React.ReactNode;
    let footer: React.ReactNode;

    if (installState.kind === "installed") {
        body = (
            <div style={{
                minHeight: 220,
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                justifyContent: "center",
                textAlign: "center",
                padding: "24px 0",
            }}>
                <Icon icon="tick-circle" size={48} intent="success" />
                <h3 style={{ marginTop: 16, marginBottom: 8, fontFamily: "var(--font-mono)", letterSpacing: 1 }}>
                    WinCommander Pro v{installState.version} installed
                </h3>
                <p style={{ color: "var(--color-text-muted)", fontSize: 13 }}>
                    Paid features are now unlocked. Click any locked toggle to start using them.
                </p>
            </div>
        );
        footer = (
            <Button className="wc-btn-primary" onClick={() => { reset(); onClose(); }}>
                Done
            </Button>
        );
    } else if (installState.kind === "error") {
        // Per-stage actionable copy. Order matters: defender_exclusion
        // is the most common failure (tamper protection / non-elevated)
        // and we want the user to know what to do, not just see a
        // mysterious PS error.
        const stage = installState.stage;
        let title = "Install failed";
        let hint: React.ReactNode = null;
        if (stage === "defender_exclusion") {
            title = "Couldn't add Defender exclusion";
            hint = (
                <p style={{ fontSize: 12, marginBottom: 12 }}>
                    The most common cause is{" "}
                    <strong>Defender Tamper Protection</strong> being on, which blocks
                    <code style={{ margin: "0 4px" }}>Add-MpPreference</code> even from elevated
                    PowerShell. Open <strong>Windows Security → Virus &amp; threat protection →
                    Manage settings</strong>, switch <strong>Tamper Protection</strong> off, then
                    click Try again.
                </p>
            );
        } else if (stage === "download") {
            title = "Couldn't download Pro";
            hint = (
                <p style={{ fontSize: 12, marginBottom: 12 }}>
                    Network failure or the manifest URL is unreachable. Check your connection,
                    then click Try again.
                </p>
            );
        } else if (stage === "sha256_mismatch") {
            title = "Pro binary failed integrity check";
            hint = (
                <p style={{ fontSize: 12, marginBottom: 12 }}>
                    The downloaded file's SHA-256 didn't match what the release manifest claims.
                    This usually means a corrupted download or, in rare cases, a
                    man-in-the-middle. We refused to install it. Click Try again.
                </p>
            );
        } else if (stage === "disk") {
            title = "Couldn't write the Pro binary";
            hint = (
                <p style={{ fontSize: 12, marginBottom: 12 }}>
                    File-system error. If wincommander-pro.exe is currently running, close it
                    first, then click Try again.
                </p>
            );
        }

        body = (
            <div style={{ padding: "8px 0" }}>
                <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 12 }}>
                    <Icon icon="error" size={24} intent="danger" />
                    <h3 style={{ margin: 0 }}>{title}</h3>
                </div>
                {hint}
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
                    [{stage}] {installState.message}
                </pre>
            </div>
        );
        footer = (
            <>
                <Button minimal onClick={async () => { await refresh(); reset(); }}>Try again</Button>
                <Button className="wc-btn-ghost" onClick={onClose}>Close</Button>
            </>
        );
    } else if (isBusy || autoProgress) {
        // isBusy = install in flight. autoProgress = combined-flow idle frame
        // before useUpdateFlow fires the auto-install — render the same plain
        // progress (consent was captured upfront) instead of the consent gate.
        body = (
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
                <h3 style={{ marginTop: 16, marginBottom: 8 }}>
                    Installing WinCommander Pro{manifest ? ` v${manifest.version}` : ""}…
                </h3>
                <p style={{ color: "var(--color-text-muted)", fontSize: 12 }}>
                    Adding Defender exclusion · Downloading · Verifying SHA-256 · Writing to disk
                </p>
            </div>
        );
        footer = (
            <Button minimal disabled>Please wait…</Button>
        );
    } else {
        // Default: consent state.
        body = (
            <div>
                <p style={{ marginBottom: 12, fontSize: 13 }}>
                    WinCommander Pro is the paid sidecar that unlocks Privacy Clean tools,
                    encrypted vault, mesh VPN, and the rest of the paid feature set. It runs
                    headless next to WinCommander.
                </p>

                {manifest && (
                    <p
                        style={{
                            fontFamily: "var(--font-mono)",
                            fontSize: 11,
                            color: "var(--color-text-muted)",
                            marginBottom: 16,
                        }}
                    >
                        v{manifest.version}
                        {manifest.size ? ` · ${(manifest.size / 1024 / 1024).toFixed(1)} MB` : ""}
                        {" · SHA-256 verified before install"}
                    </p>
                )}

                {manifestError && !manifest && (
                    <div
                        style={{
                            padding: 12,
                            border: "1px solid var(--color-danger)",
                            background: "var(--color-danger-dim)",
                            color: "var(--color-danger)",
                            marginBottom: 16,
                            fontSize: 12,
                        }}
                    >
                        Couldn't fetch the Pro release manifest:
                        <pre style={{ marginTop: 6, fontSize: 10, fontFamily: "var(--font-mono)" }}>
                            {manifestError}
                        </pre>
                        <span style={{ fontSize: 11 }}>
                            Contact <a href="mailto:[bugreports@servalabs.com]">[bugreports@servalabs.com]</a> if the issue
                            persists.
                        </span>
                    </div>
                )}

                {/* Tamper Protection pre-flight: when on, Add-MpPreference
                    will fail no matter what we do. Block Install and tell
                    the user how to disable it. */}
                {tamperOn && (
                    <div
                        style={{
                            padding: 12,
                            border: "1px solid var(--color-danger)",
                            background: "var(--color-danger-dim)",
                            marginBottom: 16,
                            fontSize: 12,
                            color: "var(--color-danger)",
                        }}
                    >
                        <strong>Defender Tamper Protection is on.</strong> Windows blocks
                        programmatic exclusion changes while it's enabled, so this install will
                        fail until you turn it off:
                        <ol style={{ margin: "8px 0 4px 18px", color: "var(--color-text-primary)" }}>
                            <li>Open <strong>Windows Security</strong></li>
                            <li>Go to <strong>Virus &amp; threat protection</strong> → <strong>Manage settings</strong></li>
                            <li>Switch <strong>Tamper Protection</strong> to <em>Off</em></li>
                            <li>
                                Come back here and click{" "}
                                <a
                                    href="#"
                                    onClick={(e) => { e.preventDefault(); void refresh(); }}
                                    style={{ color: "var(--color-accent)" }}
                                >
                                    Re-check
                                </a>
                            </li>
                        </ol>
                        <span style={{ fontSize: 11, color: "var(--color-text-muted)" }}>
                            You can re-enable Tamper Protection after the install — the
                            exclusion path persists across the toggle.
                        </span>
                    </div>
                )}

                {exclusionAlreadySet && !tamperOn && (
                    <div
                        style={{
                            padding: 10,
                            border: "1px solid var(--color-success)",
                            background: "var(--color-success-dim)",
                            marginBottom: 12,
                            fontSize: 12,
                            color: "var(--color-success)",
                        }}
                    >
                        <Icon icon="tick" size={12} style={{ marginRight: 6 }} />
                        <code style={{ margin: "0 2px" }}>%ProgramData%\WinCommander\bin\</code> is
                        already in Defender's exclusion list — install will skip that step.
                    </div>
                )}

                <div
                    style={{
                        padding: 12,
                        border: "1px solid var(--color-warning)",
                        background: "var(--color-warning-dim)",
                        marginBottom: 16,
                        fontSize: 12,
                    }}
                >
                    <strong style={{ color: "var(--color-warning)" }}>
                        Defender will flag this code.
                    </strong>{" "}
                    WinCommander Pro contains Privacy Clean features (shadow-copy clearer,
                    cipher /W overwrite, secure deletion) that look like malware to Windows
                    Defender / SmartScreen / most AVs. To install successfully, the app
                    needs to add{" "}
                    <code style={{ margin: "0 2px" }}>%ProgramData%\WinCommander\bin\</code>{" "}
                    to Defender's exclusion list before downloading the EXE.
                </div>

                <Checkbox
                    checked={consent}
                    onChange={(e) => onConsentChange((e.target as HTMLInputElement).checked)}
                    label="I understand and consent to the Defender exclusion."
                />
            </div>
        );
        footer = (
            <>
                <Button minimal className="wc-btn-ghost" onClick={onNotNow ?? onClose}>
                    Not now
                </Button>
                <Button
                    className="wc-btn-primary"
                    disabled={!consent || !manifest || tamperOn}
                    onClick={() => install(consent)}
                    title={tamperOn ? "Disable Defender Tamper Protection first" : undefined}
                >
                    Install Pro
                </Button>
            </>
        );
    }

    return { body, footer, isBusy };
}
