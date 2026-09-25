import { useEffect, useState } from "react";
import SectionCard from "../shared/SectionCard";
import UniversalToggle from "../shared/UniversalToggle";
import useBackend from "../../hooks/useBackend";
import { useAppState } from "../../context/AppContext";
import { getDisplayBranding } from "../../lib/branding";
import { showError } from "../../utils/toast";

/**
 * WinCommander right-click integrations: Shred (secure single-pass
 * delete, user-configurable up to 7 passes) + Scrub (strip EXIF / GPS /
 * author metadata). Both write to
 *   HKCU\Software\Classes\<Type>\shell\WinCommander*
 * via Tauri commands toggle_context_menu / toggle_scrub_context_menu.
 *
 * Lives in its own component because (a) it's a coherent unit, and (b)
 * keeping it inline in TweaksPanel meant duplicated registry-state +
 * search-filter logic every render. The panel now just renders this and
 * passes the search query so the card can self-hide when filtered out.
 */
export default function ContextMenuIntegrationCard({
    isAdvanced,
    searchQuery,
}: {
    isAdvanced: boolean;
    searchQuery: string;
}) {
    const { appSettings, patchAppSettings } = useAppState();
    const { productName } = getDisplayBranding(appSettings);
    const {
        toggleContextMenu, getContextMenuStatus,
        toggleScrubContextMenu, getScrubContextMenuStatus,
        toggleSafeCopyContextMenu, getSafeCopyContextMenuStatus,
    } = useBackend();

    const [contextMenuEnabled, setContextMenuEnabled] = useState(false);
    const [scrubContextMenuEnabled, setScrubContextMenuEnabled] = useState(false);
    const [safeCopyContextMenuEnabled, setSafeCopyContextMenuEnabled] = useState(false);
    const [safeCopyNotificationsEnabled, setSafeCopyNotificationsEnabled] = useState(true);
    const [loadingShred, setLoadingShred] = useState(false);
    const [loadingScrub, setLoadingScrub] = useState(false);
    const [loadingSafeCopy, setLoadingSafeCopy] = useState(false);
    const [savingSafeCopyNotifications, setSavingSafeCopyNotifications] = useState(false);

    useEffect(() => {
        if (appSettings?.app?.contextMenuEnabled != null) {
            setContextMenuEnabled(appSettings.app.contextMenuEnabled);
        }
        if (appSettings?.app?.scrubContextMenuEnabled != null) {
            setScrubContextMenuEnabled(appSettings.app.scrubContextMenuEnabled);
        }
        if (appSettings?.app?.safeCopyContextMenuEnabled != null) {
            setSafeCopyContextMenuEnabled(appSettings.app.safeCopyContextMenuEnabled);
        }
        getContextMenuStatus()
            .then(setContextMenuEnabled)
            .catch(e => console.error("Failed to check context menu status", e));
        getScrubContextMenuStatus()
            .then(setScrubContextMenuEnabled)
            .catch(e => console.error("Failed to check scrub context menu status", e));
        getSafeCopyContextMenuStatus()
            .then(setSafeCopyContextMenuEnabled)
            .catch(e => console.error("Failed to check safe copy context menu status", e));
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    useEffect(() => {
        if (appSettings?.app?.safeCopyNotificationsEnabled != null) {
            setSafeCopyNotificationsEnabled(appSettings.app.safeCopyNotificationsEnabled);
        }
    }, [appSettings?.app?.safeCopyNotificationsEnabled]);

    const handleShredToggle = async (checked: boolean) => {
        setContextMenuEnabled(checked);
        setLoadingShred(true);
        try {
            await toggleContextMenu(checked);
            const actual = await getContextMenuStatus();
            setContextMenuEnabled(actual);
            await patchAppSettings({ app: { contextMenuEnabled: actual } });
        } catch (e) {
            setContextMenuEnabled(!checked);
            const msg = e instanceof Error ? e.message : String(e);
            if (msg) showError(`Context menu toggle failed: ${msg}`);
            console.error(e);
        } finally {
            setLoadingShred(false);
        }
    };

    const handleScrubToggle = async (checked: boolean) => {
        setScrubContextMenuEnabled(checked);
        setLoadingScrub(true);
        try {
            await toggleScrubContextMenu(checked);
            const actual = await getScrubContextMenuStatus();
            setScrubContextMenuEnabled(actual);
            await patchAppSettings({ app: { scrubContextMenuEnabled: actual } });
        } catch (e) {
            setScrubContextMenuEnabled(!checked);
            const msg = e instanceof Error ? e.message : String(e);
            if (msg) showError(`Scrub toggle failed: ${msg}`);
            console.error(e);
        } finally {
            setLoadingScrub(false);
        }
    };

    const handleSafeCopyToggle = async (checked: boolean) => {
        setSafeCopyContextMenuEnabled(checked);
        setLoadingSafeCopy(true);
        try {
            await toggleSafeCopyContextMenu(checked);
            const actual = await getSafeCopyContextMenuStatus();
            setSafeCopyContextMenuEnabled(actual);
            await patchAppSettings({ app: { safeCopyContextMenuEnabled: actual } });
        } catch (e) {
            setSafeCopyContextMenuEnabled(!checked);
            const msg = e instanceof Error ? e.message : String(e);
            if (msg) showError(`Safe Copy/Paste toggle failed: ${msg}`);
            console.error(e);
        } finally {
            setLoadingSafeCopy(false);
        }
    };

    const handleSafeCopyNotificationsToggle = async (checked: boolean) => {
        const previous = appSettings?.app?.safeCopyNotificationsEnabled ?? true;
        setSafeCopyNotificationsEnabled(checked);
        setSavingSafeCopyNotifications(true);
        try {
            await patchAppSettings({ app: { safeCopyNotificationsEnabled: checked } });
        } catch (e) {
            setSafeCopyNotificationsEnabled(previous);
            const msg = e instanceof Error ? e.message : String(e);
            if (msg) showError(`Safe Copy notification setting failed: ${msg}`);
            console.error(e);
        } finally {
            setSavingSafeCopyNotifications(false);
        }
    };

    // Search-aware self-hiding: only render the rows that match the
    // panel's global search query.
    const q = searchQuery.toLowerCase().trim();
    const matchesShred = "context menu delete".includes(q) || "right-click delete".includes(q) || "delete".includes(q);
    const matchesScrub = "scrub".includes(q) || "scrub metadata".includes(q);
    const matchesSafeCopy = "safe copy".includes(q) || "safe paste".includes(q) || "clean copy".includes(q)
        || "safe copy notifications".includes(q) || "copy notifications".includes(q);
    const matchesSection = "context menu".includes(q) || "right-click".includes(q) || "explorer integration".includes(q);
    const showShred = !q || matchesShred || matchesSection;
    const showScrub = !q || matchesScrub || matchesSection;
    const showSafeCopy = !q || matchesSafeCopy || matchesSection;
    if (q && !showShred && !showScrub && !showSafeCopy) return null;

    return (
        <SectionCard
            title={isAdvanced ? "Context Menu Integration" : `${productName} shortcuts`}
            icon="properties"
        >
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
                <div
                    style={{
                        fontSize: 12,
                        color: 'var(--color-text-secondary)',
                        lineHeight: 1.5,
                    }}
                >
                    Add {productName} tools to the Windows right-click menu so you
                    can act on files without opening the app first. Each toggle
                    writes its own registry entry under <code>HKCU\Software\Classes</code> —
                    no admin needed, applies per-user.
                </div>
                {showShred && (
                    <UniversalToggle
                        label="Secure Delete"
                        description="Add a separate secure-shred action to right-click on selected files and folders. It permanently deletes the selected items; Windows' normal Delete remains unchanged."
                        icon="trash"
                        checked={contextMenuEnabled}
                        onChange={handleShredToggle}
                        disabled={loadingShred}
                    />
                )}
                {showScrub && (
                    <UniversalToggle
                        label="Scrub metadata"
                        description="Add 'Scrub metadata' to right-click. Strips EXIF / GPS / author info before sharing photos, videos, and documents."
                        icon="eraser"
                        checked={scrubContextMenuEnabled}
                        onChange={handleScrubToggle}
                        disabled={loadingScrub}
                    />
                )}
                {showSafeCopy && (
                    <>
                        <UniversalToggle
                            label="Safe Copy / Safe Paste"
                            description="Add 'Safe Copy' + 'Safe Paste' to right-click. Safe Copy scrubs a private cache, then places only clean copies on the Windows clipboard; normal Paste and Safe Paste retain the original filenames. (Metadata scrub is a Pro feature.)"
                            icon="clipboard"
                            checked={safeCopyContextMenuEnabled}
                            onChange={handleSafeCopyToggle}
                            disabled={loadingSafeCopy}
                        />
                        <UniversalToggle
                            label="Safe Copy notifications"
                            description="Show a WinCommander notification while Safe Copy scrubs and when the cleaned clipboard is ready, including elapsed time. Turn off to copy silently."
                            icon="notifications"
                            checked={safeCopyNotificationsEnabled}
                            onChange={handleSafeCopyNotificationsToggle}
                            disabled={savingSafeCopyNotifications}
                        />
                    </>
                )}
            </div>
        </SectionCard>
    );
}
