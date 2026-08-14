// src/panels/secret/VisibilityTable.tsx
//
// 3-way visibility table for the Borrowed Mode section.
// Rows: every non-chip-excluded panel + feature rows (notif bell,
//   risk matrix, more products, sidebar actions).
// Axes: Visible | Hide in Borrowed | Hide Always
//
// Panel rows wire app.lockedPanelIds (borrowed) and
// app.permanentlyHiddenPanels (always).
// Feature/action rows wire app.borrowedHidden (borrowed) and their
// own existing always-hidden fields (always).

import { useCallback, useEffect, useMemo, useState } from "react";
import { useAppState } from "../../context/AppContext";
import useEntitlements from "../../hooks/useEntitlements";
import { Icon } from "@/components/ui/bp";
import { PANEL_MANIFESTS, NAV_GROUP_ORDER, navGroupFor, type PanelId } from "../../types/panels";
import { DEFAULT_ALWAYS_PANELS, DEFAULT_BORROWED_PANELS, DEFAULT_BORROWED_EXTRAS } from "../../lib/visibilityDefaults";
import {
    getPopupAlertsEnabled,
    setPopupAlertsEnabled,
    POPUP_PREF_CHANGED_EVENT,
} from "../../lib/notificationStore";

// "secret" is governed by its own 5×-click reveal gate, not this table; the
// dashboard / file-search / advisor / flows surfaces aren't user-hideable here.
const EXCLUDED_IDS: PanelId[] = ["dashboard", "search-files", "advisor", "secret"];

// Order rows the same way the sidebar lists them: by nav-group order
// (Monitor → Protect → Secure → System → footer), then by panel `order`
// within each group. Keeps this table in lock-step with the rail.
const GROUP_INDEX: Record<string, number> = Object.fromEntries(
    NAV_GROUP_ORDER.map((g, i) => [g.id, i]),
);

const TABLE_PANELS = PANEL_MANIFESTS
    .filter(p => !EXCLUDED_IDS.includes(p.id as PanelId))
    .sort((a, b) => {
        const ga = GROUP_INDEX[navGroupFor(a.id)] ?? 99;
        const gb = GROUP_INDEX[navGroupFor(b.id)] ?? 99;
        return ga !== gb ? ga - gb : a.order - b.order;
    })
    .map(p => ({ id: p.id as PanelId, label: p.label, icon: p.icon }));

type VisState = "visible" | "borrowed" | "always";

function getVis(id: string, locked: string[], perm: string[]): VisState {
    if (perm.includes(id)) return "always";
    if (locked.includes(id)) return "borrowed";
    return "visible";
}

// Pure helper: set/remove a key from a borrowedHidden array and return the
// next array. Takes the array as an argument (rather than closing over
// component state) so callers can apply it against the freshest settings
// snapshot at write time — see the patchAppSettings updater-function calls
// below for why that matters.
function withBorrowed(current: string[], key: string, add: boolean): string[] {
    const s = new Set(current);
    if (add) s.add(key); else s.delete(key);
    return [...s];
}

// ── Segmented control ─────────────────────────────────────────────────────

interface SegProps {
    value: VisState;
    allowBorrowed?: boolean;
    onSelect: (v: VisState) => void;
}

function VisSegment({ value, allowBorrowed = true, onSelect }: SegProps) {
    // The control answers "Hide this?": No = always shown · When borrowed =
    // hidden only while Borrowed Mode is active · Always = hidden everywhere.
    const segments: { id: VisState; label: string }[] = [
        { id: "visible", label: "No" },
        { id: "borrowed", label: "When borrowed" },
        { id: "always", label: "Always" },
    ];
    return (
        <div className="vis-seg">
            {segments.map(s => {
                const isDisabled = s.id === "borrowed" && !allowBorrowed;
                const isActive = value === s.id;
                return (
                    <button
                        key={s.id}
                        type="button"
                        className={`vis-seg-btn ${isActive ? "active" : ""} ${isDisabled ? "disabled" : ""}`}
                        disabled={isDisabled}
                        title={isDisabled ? "Panel-level only (coming soon)" : undefined}
                        onClick={() => !isDisabled && onSelect(s.id)}
                    >
                        {s.label}
                    </button>
                );
            })}
        </div>
    );
}

// ── Main table ────────────────────────────────────────────────────────────

export default function VisibilityTable() {
    const { appSettings, patchAppSettings } = useAppState();
    const { hasPaid } = useEntitlements();

    // Memoize derived arrays so useCallback deps are stable
    const locked = useMemo(
        () => (appSettings?.app?.lockedPanelIds ?? DEFAULT_BORROWED_PANELS) as string[],
        [appSettings?.app?.lockedPanelIds],
    );
    const perm = useMemo(
        () => (appSettings?.app?.permanentlyHiddenPanels ?? DEFAULT_ALWAYS_PANELS) as string[],
        [appSettings?.app?.permanentlyHiddenPanels],
    );
    const hiddenActions = useMemo(
        () => (appSettings?.app?.hiddenSidebarActions ?? []) as string[],
        [appSettings?.app?.hiddenSidebarActions],
    );
    const borrowedHidden = useMemo(
        () => (appSettings?.app?.borrowedHidden ?? DEFAULT_BORROWED_EXTRAS) as string[],
        [appSettings?.app?.borrowedHidden],
    );

    const notifHidden = appSettings?.app?.hideNotificationBell === true;
    const riskMatrixHidden = appSettings?.ideal?.identity?.riskMatrixEnabled === false;
    const moreProdsHidden = appSettings?.ideal?.identity?.moreProductsEnabled === false;
    const desktopAlertsDisabled = appSettings?.app?.disableNativeNotifications === true;
    const enginesHidden = appSettings?.app?.hideEnginesSection === true;
    const licenseHidden = appSettings?.app?.hideLicensePanel === true;
    const preferencesHidden = appSettings?.app?.hideSidebarPreferences === true;
    const tourHidden = appSettings?.app?.hideTour === true;

    // Pop-up alerts pref lives in localStorage; sync into React state.
    const [popupAlertsOn, setPopupAlertsOn] = useState<boolean>(() => getPopupAlertsEnabled());
    useEffect(() => {
        const sync = () => setPopupAlertsOn(getPopupAlertsEnabled());
        window.addEventListener(POPUP_PREF_CHANGED_EVENT, sync);
        return () => window.removeEventListener(POPUP_PREF_CHANGED_EVENT, sync);
    }, []);

    // KT (perf/correctness): every handler below reads shared array fields
    // (lockedPanelIds, permanentlyHiddenPanels, borrowedHidden, hiddenSidebarActions)
    // via the patchAppSettings *updater* form — `(latest) => patch` — instead of
    // closing over the memoized `locked`/`perm`/`borrowedHidden`/`hiddenActions`
    // above. The backend replaces these arrays wholesale (no per-element merge),
    // so if two rows in this table are toggled in quick succession, the second
    // write's patch — built from a still-stale pre-render snapshot — would
    // otherwise silently revert the first row's just-written change once its
    // write's turn came up in the serialized patch queue. The updater form is
    // resolved at write time against the freshest settings instead.
    const setPanelVis = useCallback((id: PanelId, vis: VisState) => {
        // Persist only. This is CONFIG, not a mode switch — the sidebar re-reads
        // lockedPanelIds / permanentlyHiddenPanels reactively. Do NOT dispatch
        // hidden-panels-lock/unlock here: those toggle Borrowed Mode at runtime,
        // and lock would navigate away from (and hide) this Secret Settings panel.
        patchAppSettings((latest) => {
            const lockedNow = (latest?.app?.lockedPanelIds ?? DEFAULT_BORROWED_PANELS) as string[];
            const permNow = (latest?.app?.permanentlyHiddenPanels ?? DEFAULT_ALWAYS_PANELS) as string[];
            const nextLocked = lockedNow.filter(x => x !== id);
            const nextPerm = permNow.filter(x => x !== id);
            if (vis === "borrowed") nextLocked.push(id);
            if (vis === "always") nextPerm.push(id);
            return { app: { lockedPanelIds: nextLocked, permanentlyHiddenPanels: nextPerm } };
        }).catch(() => {});
    }, [patchAppSettings]);

    const toggleNotif = useCallback((vis: VisState) => {
        patchAppSettings((latest) => ({
            app: {
                hideNotificationBell: vis === "always",
                borrowedHidden: withBorrowed((latest?.app?.borrowedHidden ?? DEFAULT_BORROWED_EXTRAS) as string[], "notif-bell", vis === "borrowed"),
            },
        })).catch(() => {});
    }, [patchAppSettings]);

    const toggleRiskMatrix = useCallback((vis: VisState) => {
        if (!hasPaid) return;
        patchAppSettings((latest) => ({
            ideal: { identity: { riskMatrixEnabled: vis !== "always" } },
            app: { borrowedHidden: withBorrowed((latest?.app?.borrowedHidden ?? DEFAULT_BORROWED_EXTRAS) as string[], "risk-matrix", vis === "borrowed") },
        })).catch(() => {});
    }, [hasPaid, patchAppSettings]);

    const toggleMoreProds = useCallback((vis: VisState) => {
        patchAppSettings((latest) => ({
            ideal: { identity: { moreProductsEnabled: vis !== "always" } },
            app: { borrowedHidden: withBorrowed((latest?.app?.borrowedHidden ?? DEFAULT_BORROWED_EXTRAS) as string[], "more-products", vis === "borrowed") },
        })).catch(() => {});
    }, [patchAppSettings]);

    const toggleSidebarAction = useCallback((key: string, vis: VisState) => {
        patchAppSettings((latest) => {
            const hiddenNow = (latest?.app?.hiddenSidebarActions ?? []) as string[];
            const next = new Set(hiddenNow);
            if (vis === "always") next.add(key); else next.delete(key);
            return {
                app: {
                    hiddenSidebarActions: [...next],
                    borrowedHidden: withBorrowed((latest?.app?.borrowedHidden ?? DEFAULT_BORROWED_EXTRAS) as string[], `action:${key}`, vis === "borrowed"),
                },
            };
        }).catch(() => {});
    }, [patchAppSettings]);

    const togglePopupAlerts = useCallback((vis: VisState) => {
        setPopupAlertsEnabled(vis !== "always");
        patchAppSettings((latest) => ({
            app: { borrowedHidden: withBorrowed((latest?.app?.borrowedHidden ?? DEFAULT_BORROWED_EXTRAS) as string[], "popup-alerts", vis === "borrowed") },
        })).catch(() => {});
    }, [patchAppSettings]);

    const toggleDesktopAlerts = useCallback((vis: VisState) => {
        patchAppSettings((latest) => ({
            app: {
                disableNativeNotifications: vis === "always",
                borrowedHidden: withBorrowed((latest?.app?.borrowedHidden ?? DEFAULT_BORROWED_EXTRAS) as string[], "desktop-alerts", vis === "borrowed"),
            },
        } as any)).catch(() => {});
    }, [patchAppSettings]);

    const toggleEnginesSection = useCallback((vis: VisState) => {
        patchAppSettings((latest) => ({
            app: {
                hideEnginesSection: vis === "always",
                borrowedHidden: withBorrowed((latest?.app?.borrowedHidden ?? DEFAULT_BORROWED_EXTRAS) as string[], "engines-section", vis === "borrowed"),
            },
        } as any)).catch(() => {});
    }, [patchAppSettings]);

    const toggleSidebarPreferencesAndLicense = useCallback((vis: VisState) => {
        patchAppSettings((latest) => ({
            app: {
                hideLicensePanel: vis === "always",
                hideSidebarPreferences: vis === "always",
                borrowedHidden: withBorrowed(
                    withBorrowed(
                        (latest?.app?.borrowedHidden ?? DEFAULT_BORROWED_EXTRAS) as string[],
                        "license-panel",
                        vis === "borrowed",
                    ),
                    "sidebar-preferences",
                    vis === "borrowed",
                ),
            },
        } as any)).catch(() => {});
    }, [patchAppSettings]);

    const toggleTour = useCallback((vis: VisState) => {
        patchAppSettings((latest) => ({
            app: {
                hideTour: vis === "always",
                borrowedHidden: withBorrowed((latest?.app?.borrowedHidden ?? DEFAULT_BORROWED_EXTRAS) as string[], "tour", vis === "borrowed"),
            },
        } as any)).catch(() => {});
    }, [patchAppSettings]);

    return (
        <div className="vis-table">
            {/* Header */}
            <div className="vis-table-header">
                <span className="vis-col-label">Item</span>
                <span className="vis-col-ctrl">Hide this?</span>
            </div>

            {/* Panel rows — 2-column grid, sidebar order */}
            <div className="vis-table-group-label">Panels</div>
            <div className="vis-row-grid">
                {TABLE_PANELS.map(({ id, label, icon }) => (
                    <div key={id} className="vis-table-row">
                        <span className="vis-row-label">
                            <Icon icon={icon} size={12} />
                            {label}
                        </span>
                        <VisSegment
                            value={getVis(id, locked, perm)}
                            onSelect={v => setPanelVis(id, v)}
                        />
                    </div>
                ))}
            </div>

            {/* Feature rows — full 3-state (Visible | Borrowed | Always) */}
            <div className="vis-table-group-label">UI Features</div>
            <div className="vis-row-grid">
                <div className="vis-table-row">
                    <span className="vis-row-label">
                        <Icon icon="notifications" size={12} />
                        Alerts &amp; Processes icons
                    </span>
                    <VisSegment
                        value={notifHidden ? "always" : borrowedHidden.includes("notif-bell") ? "borrowed" : "visible"}
                        allowBorrowed={true}
                        onSelect={toggleNotif}
                    />
                </div>

                <div className="vis-table-row">
                    <span className="vis-row-label">
                        <Icon icon="shield" size={12} />
                        Dashboard Risk Matrix
                        {!hasPaid && <span className="vis-pro-badge">PRO</span>}
                    </span>
                    <VisSegment
                        value={riskMatrixHidden ? "always" : borrowedHidden.includes("risk-matrix") ? "borrowed" : "visible"}
                        allowBorrowed={true}
                        onSelect={toggleRiskMatrix}
                    />
                </div>

                <div className="vis-table-row">
                    <span className="vis-row-label">
                        <Icon icon="clean" size={12} />
                        Dashboard More Products
                    </span>
                    <VisSegment
                        value={moreProdsHidden ? "always" : borrowedHidden.includes("more-products") ? "borrowed" : "visible"}
                        allowBorrowed={true}
                        onSelect={toggleMoreProds}
                    />
                </div>

                <div className="vis-table-row">
                    <span className="vis-row-label">
                        <Icon icon="notifications" size={12} />
                        Pop-up alerts
                    </span>
                    <VisSegment
                        value={!popupAlertsOn ? "always" : borrowedHidden.includes("popup-alerts") ? "borrowed" : "visible"}
                        allowBorrowed={true}
                        onSelect={togglePopupAlerts}
                    />
                </div>

                <div className="vis-table-row">
                    <span className="vis-row-label">
                        <Icon icon="desktop" size={12} />
                        Desktop alerts
                    </span>
                    <VisSegment
                        value={desktopAlertsDisabled ? "always" : borrowedHidden.includes("desktop-alerts") ? "borrowed" : "visible"}
                        allowBorrowed={true}
                        onSelect={toggleDesktopAlerts}
                    />
                </div>

                <div className="vis-table-row">
                    <span className="vis-row-label">
                        <Icon icon="cog" size={12} />
                        Engines section
                    </span>
                    <VisSegment
                        value={enginesHidden ? "always" : borrowedHidden.includes("engines-section") ? "borrowed" : "visible"}
                        allowBorrowed={true}
                        onSelect={toggleEnginesSection}
                    />
                </div>

                <div className="vis-table-row">
                    <span className="vis-row-label">
                        <Icon icon="cog" size={12} />
                        Persona, interface &amp; licensing
                    </span>
                    <VisSegment
                        value={preferencesHidden || licenseHidden
                            ? "always"
                            : borrowedHidden.includes("sidebar-preferences") || borrowedHidden.includes("license-panel")
                                ? "borrowed"
                                : "visible"}
                        allowBorrowed={true}
                        onSelect={toggleSidebarPreferencesAndLicense}
                    />
                </div>

                <div className="vis-table-row">
                    <span className="vis-row-label">
                        <Icon icon="help" size={12} />
                        Tour &amp; guide
                    </span>
                    <VisSegment
                        value={tourHidden ? "always" : borrowedHidden.includes("tour") ? "borrowed" : "visible"}
                        allowBorrowed={true}
                        onSelect={toggleTour}
                    />
                </div>
            </div>

            {/* Sidebar action rows — full 3-state */}
            <div className="vis-table-group-label">Sidebar Quick Actions</div>
            <div className="vis-row-grid">
                {[
                    { key: "ai-advisor", label: "AI Advisor", icon: "predictive-analysis" as const },
                    { key: "search", label: "File Search", icon: "search" as const },
                    { key: "dismount", label: "Dismount volumes", icon: "eject" as const },
                    { key: "delete", label: "Secure Shredder", icon: "trash" as const },
                    { key: "scrubMeta", label: "Scrub Metadata", icon: "eraser" as const },
                    { key: "lockdown", label: "Lockdown", icon: "warning-sign" as const },
                ].map(({ key, label, icon }) => (
                    <div key={key} className="vis-table-row">
                        <span className="vis-row-label">
                            <Icon icon={icon} size={12} />
                            {label}
                        </span>
                        <VisSegment
                            value={hiddenActions.includes(key) ? "always" : borrowedHidden.includes(`action:${key}`) ? "borrowed" : "visible"}
                            allowBorrowed={true}
                            onSelect={v => toggleSidebarAction(key, v)}
                        />
                    </div>
                ))}
            </div>
        </div>
    );
}
