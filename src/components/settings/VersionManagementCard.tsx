import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { Button, Icon, Switch } from "@/components/ui/bp";
import SectionCard from "../shared/SectionCard";
import { useAppConfirm } from "../shared/AppConfirmDialog";
import UpdateFlowDialog from "../UpdateFlowDialog";
import useEntitlements from "../../hooks/useEntitlements";
import useProInstall from "../../hooks/useProInstall";
import { showError, showSuccess, showWarning } from "../../utils/toast";
import { useAppState } from "../../context/AppContext";
import "./VersionManagementCard.css";

interface FreeUpdateInfo {
    available: boolean;
    version?: string | null;
    current_version?: string | null;
}

function compareVersions(a: string | null | undefined, b: string | null | undefined): number | null {
    const parse = (value: string | null | undefined): number[] | null => {
        const match = (value ?? "").trim().replace(/^v/i, "").match(/\d+(?:\.\d+){0,3}/);
        if (!match) return null;
        return match[0].split(".").map((part) => Number.parseInt(part, 10));
    };
    const left = parse(a);
    const right = parse(b);
    if (!left || !right) return null;
    const len = Math.max(left.length, right.length);
    for (let i = 0; i < len; i++) {
        const diff = (left[i] ?? 0) - (right[i] ?? 0);
        if (diff !== 0) return diff > 0 ? 1 : -1;
    }
    return 0;
}

function VersionRow({
    label,
    current,
    latest,
    status,
    tone,
    actions,
}: {
    label: string;
    current: string;
    latest: string;
    status: string;
    tone: "ok" | "update" | "muted";
    actions?: ReactNode;
}) {
    return (
        <div className="version-row">
            <div className="version-row__name">{label}</div>
            <div className="version-row__values">
                <div className="version-row__version">
                    <span className="version-row__label">Current</span>
                    <span>{current}</span>
                </div>
                <div className="version-row__version">
                    <span className="version-row__label">Latest</span>
                    <span>{latest}</span>
                </div>
            </div>
            <div className="version-row__end">
                <span className={`version-row__badge version-row__badge--${tone}`}>
                    {status}
                </span>
                {actions}
            </div>
        </div>
    );
}

export default function VersionManagementCard() {
    const [freeCurrent, setFreeCurrent] = useState<string | null>(null);
    const [freeLatest, setFreeLatest] = useState<string | null>(null);
    const [freeChecking, setFreeChecking] = useState(false);
    const [updateFlowOpen, setUpdateFlowOpen] = useState(false);
    const { hasPaid } = useEntitlements();
    const { appSettings, patchAppSettings } = useAppState();
    const automaticUpdatesEnabled = appSettings?.app?.autoUpdate ?? true;

    const refreshVersions = useCallback(async () => {
        setFreeChecking(true);
        try {
            const [current, update] = await Promise.all([
                getVersion().catch(() => null),
                invoke<FreeUpdateInfo>("app_check_for_updates_doh"),
            ]);
            const installed = update.current_version ?? current;
            setFreeCurrent(installed);
            setFreeLatest(update.version ?? installed);
        } catch (err) {
            const message = err instanceof Error ? err.message : String(err);
            // Update result → Notifications tab, not System Alerts.
            showWarning(`Free update check failed: ${message}`, undefined, { kind: "notification" });
            const current = await getVersion().catch(() => null);
            setFreeCurrent(current);
            setFreeLatest(current);
        } finally {
            setFreeChecking(false);
        }
    }, []);

    useEffect(() => {
        void refreshVersions();
    }, [refreshVersions]);

    const freeCompare = compareVersions(freeLatest, freeCurrent);
    const freeNeedsUpdate = freeCompare === 1;

    return (
        <SectionCard
            title="Versions & Updates"
            icon="download"
            className="version-management-card"
            headerRight={
                <Button
                    minimal
                    icon="refresh"
                    onClick={() => setUpdateFlowOpen(true)}
                    title={hasPaid
                        ? "Check for Updates — checks Free first, then Pro automatically if you're entitled — one dialog, one restart at the end."
                        : "Check for Updates"}
                    aria-label="Check for Updates"
                />
            }
        >
            <div className="version-stack">
                <div className="version-auto-update">
                    <div>
                        <div className="version-auto-update__title">Automatically update WinCommander{hasPaid ? " and Pro" : ""}</div>
                        <div className="version-auto-update__description">
                            Downloads and installs updates in the background, then restarts WinCommander when Free changes. Pro updates automatically after its one-time Defender approval. Turn this off to review each update first.
                        </div>
                    </div>
                    <Switch
                        checked={automaticUpdatesEnabled}
                        onChange={(event) => {
                            void patchAppSettings({ app: { autoUpdate: event.currentTarget.checked } });
                        }}
                        aria-label="Automatically update WinCommander"
                    />
                </div>
                {/* Check/Update/Delete controls stay available inline per-row as a
                    fallback to the combined flow above — e.g. when Pro's release is
                    ahead of the running Free version (see useUpdateFlow's
                    isManifestCompatible guard), or for repair/delete actions the
                    combined flow doesn't cover. */}
                <VersionRow
                    label="Free"
                    current={freeCurrent ?? "checking"}
                    latest={freeLatest ?? "checking"}
                    status={freeNeedsUpdate ? "Update" : freeCurrent ? "Latest" : "Check"}
                    tone={freeNeedsUpdate ? "update" : freeCurrent ? "ok" : "muted"}
                    actions={
                        <>
                            <Button
                                minimal
                                icon="refresh"
                                loading={freeChecking}
                                onClick={refreshVersions}
                                title="Check"
                                aria-label="Check Free version"
                            />
                            <Button
                                minimal
                                intent={freeNeedsUpdate ? "primary" : undefined}
                                icon="download"
                                disabled={!freeNeedsUpdate}
                                // Routes through the same combined Free->Pro flow as the
                                // header's "Check for Updates" button and the Dashboard's
                                // "Fix Everything" — calling app_install_update_doh directly
                                // here updated ONLY the Free binary, leaving a paid user's
                                // Pro sidecar to update "separately" via its own path.
                                onClick={() => setUpdateFlowOpen(true)}
                                title="Update Free"
                                aria-label="Update Free"
                            />
                        </>
                    }
                />
                {hasPaid && <ProVersionRow freeNeedsUpdate={freeNeedsUpdate} />}
            </div>

            {hasPaid && <ProVersionNotices freeNeedsUpdate={freeNeedsUpdate} />}

            <UpdateFlowDialog
                isOpen={updateFlowOpen}
                updateAvailable={freeNeedsUpdate}
                onClose={() => {
                    setUpdateFlowOpen(false);
                    // The dialog drives its own Free/Pro invokes independently
                    // of this card's local free-version state and the shared
                    // useProInstall snapshot (Pro side updates automatically
                    // via useProInstall's module-scoped store) — refresh the
                    // Free row so it reflects whatever the combined flow did.
                    void refreshVersions();
                }}
                hasPaid={hasPaid}
            />
        </SectionCard>
    );
}

function useProVersionState() {
    // This hook is only mounted by paid Pro rows inside the explicit Version
    // Management surface, so all three integrity probes are relevant here.
    const pro = useProInstall({ status: true, manifest: true, defender: true });
    const proCompare = compareVersions(pro.manifest?.version, pro.status?.local_version);
    const hashMatchesLatest = !!(
        pro.manifest?.sha256
        && pro.status?.local_sha256
        && pro.status.local_sha256.toLowerCase() === pro.manifest.sha256.toLowerCase()
    );
    const needsRepair = !!pro.status?.installed && !pro.status.local_version && !hashMatchesLatest;
    const proState: "ok" | "update" | "muted" = !pro.status?.installed
        ? "muted"
        : hashMatchesLatest
            ? "ok"
            : proCompare === 1 || !pro.status.local_version
            ? "update"
            : "ok";
    const proActionLabel = useMemo(() => {
        if (!pro.status?.installed) return "Install Pro";
        if (needsRepair) return "Repair Pro";
        return proCompare === 1 ? "Update Pro" : "Reinstall Pro";
    }, [needsRepair, proCompare, pro.status?.installed]);
    return { ...pro, proCompare, proState, hashMatchesLatest, needsRepair, proActionLabel };
}

function ProVersionRow({ freeNeedsUpdate }: { freeNeedsUpdate: boolean }) {
    const { manifest, status, proState, hashMatchesLatest, needsRepair } = useProVersionState();
    const currentVersion = status?.local_version
        ?? (
            status?.installed
            && hashMatchesLatest
            && manifest?.version
                ? manifest.version
                : status?.installed
                    ? "not verified"
                    : "none"
        );
    return (
        <VersionRow
            label="Pro"
            current={currentVersion}
            latest={manifest?.version ?? "checking"}
            status={needsRepair ? "Repair" : status?.installed ? "Installed" : "Not installed"}
            tone={status?.installed ? proState : "update"}
            actions={<ProVersionActions freeNeedsUpdate={freeNeedsUpdate} />}
        />
    );
}

function ProVersionNotices({ freeNeedsUpdate }: { freeNeedsUpdate: boolean }) {
    const { manifestError } = useProVersionState();
    return (
        <>
            {freeNeedsUpdate && (
                <div className="version-notice">
                    <Icon icon="warning-sign" size={14} />
                    Free must be updated before Pro install or reinstall.
                </div>
            )}
            {manifestError && (
                <div className="version-notice version-notice--muted">
                    Pro latest unavailable: {manifestError}
                </div>
            )}
        </>
    );
}

function ProVersionActions({ freeNeedsUpdate }: { freeNeedsUpdate: boolean }) {
    const confirmAction = useAppConfirm();
    const [deletingPro, setDeletingPro] = useState(false);
    const { manifestError, status, installState, refresh, proActionLabel, proCompare, needsRepair } = useProVersionState();
    const proBusy = installState.kind === "installing" || deletingPro;
    const proAttentionNeeded = !status?.installed || needsRepair || proCompare === 1;

    const openProInstaller = () => {
        if (freeNeedsUpdate) {
            showWarning("Update WinCommander Free first, then install or update Pro.", undefined, { kind: "notification" });
            return;
        }
        window.dispatchEvent(new CustomEvent("pro-install-open"));
    };

    const deletePro = async () => {
        const accepted = await confirmAction({
            title: "Remove the installed Pro sidecar?",
            description: "This deletes the installed WinCommander Pro binary. Pro features will be unavailable until it is installed again.",
            confirmLabel: "Remove Pro",
        });
        if (!accepted) return;
        setDeletingPro(true);
        try {
            await invoke("delete_pro_binary");
            await refresh();
            window.dispatchEvent(new CustomEvent("pro-install-state-changed"));
            showSuccess("Installed Pro sidecar removed.");
        } catch (err) {
            showError(err instanceof Error ? err.message : String(err), undefined, { kind: "notification" });
        } finally {
            setDeletingPro(false);
        }
    };

    return (
        <>
            <Button
                minimal
                intent={proAttentionNeeded ? "primary" : undefined}
                icon="cloud-download"
                loading={installState.kind === "installing"}
                disabled={freeNeedsUpdate || proBusy || !!manifestError}
                title={freeNeedsUpdate ? "Update Free first" : proActionLabel}
                aria-label={proActionLabel}
                onClick={openProInstaller}
            />
            <Button
                minimal
                intent="danger"
                icon="trash"
                loading={deletingPro}
                disabled={!status?.installed || proBusy}
                onClick={deletePro}
                title="Delete Pro"
                aria-label="Delete Pro"
            />
        </>
    );
}
