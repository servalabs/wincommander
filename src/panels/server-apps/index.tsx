// ══════════════════════════════════════════════════════════════════════════
// Server Apps Panel — Native WebView2 Multiwebview with Browser-style Tabs
// ══════════════════════════════════════════════════════════════════════════
// ARCHITECTURE: Uses Tauri v2's multiwebview (unstable feature) to embed
// server apps as native WebView2 child instances inside the main window.
// This bypasses X-Frame-Options / CSP frame-ancestors restrictions that
// block iframes for most self-hosted apps (Immich, Nextcloud, etc.).
//
// LAYOUT: Browser-style tab bar at the top (DOM), webview content below.
// The tab bar is spatially ABOVE the native webview — no z-order conflicts.
// The content area is an empty <div ref={contentRef}> used purely for
// measuring bounds. The native webview is positioned at those coordinates.
//
// LEARNING: Native child webviews render ON TOP of all DOM/CSS content.
// A sidebar approach fails because the webview covers the sidebar on hover.
// Tabs at the top avoid this since they're spatially above the webview area.
//
// LEARNING: Some self-hosted apps (Immich, Home Assistant) serve mobile UI
// when the viewport is small or UA looks non-desktop. We set a desktop
// user-agent in the Rust WebviewBuilder to force desktop layouts.
//
// CUSTOM CSS: Each app can have a `customCss` string in settings that
// gets injected via Tauri's initialization_script (runs before page JS).
// Used for whitelabeling — hiding logos, renaming headers, etc.
// ══════════════════════════════════════════════════════════════════════════

import { useState, useCallback, useEffect } from 'react';
import { Button, Icon, NonIdealState, IconName } from "@/components/ui/bp";
import { usePatchSettings, useSettingsQuery } from '../../hooks/queries/useSettingsQuery';
import EmbeddedWebView from '../../components/shared/EmbeddedWebView';
import ManageAppsDialog from './ManageAppsDialog';
import type { ServerAppConfig } from '../../types/settings';
import './index.css';

// Default configuration — used when settings.json has no server apps defined
const DEFAULT_APPS = [
    { id: 'gallery', name: 'AI Gallery', url: 'http://192.168.1.10:2283', icon: 'media', customCss: '' },
    { id: 'cloud', name: 'Private Cloud', url: 'http://192.168.1.10:8444/apps/files/files', icon: 'cloud', customCss: '' },
    { id: 'sync', name: 'Syncing App', url: 'http://192.168.1.10:8384', icon: 'refresh', customCss: '' },
    { id: 'pdf', name: 'PDF Tools', url: 'http://192.168.1.10:8080/', icon: 'document', customCss: '' },
    { id: 'firewall', name: 'Firewall', url: 'http://192.168.1.1:3000', icon: 'shield', customCss: '' },
    { id: 'controller', name: 'Smart Controller', url: 'http://192.168.1.10:8123/', icon: 'home', customCss: '' },
    { id: 'dashboard', name: 'Dashboard', url: 'http://192.168.1.30/#/', icon: 'dashboard', customCss: '' },
];

/**
 * Preserve an explicitly configured empty list. Falling back on `.length`
 * made "remove every app" impossible: the next render silently restored all
 * defaults and left the purpose-built empty state unreachable.
 */
export function resolveServerApps(configured: ServerAppConfig[] | undefined): ServerAppConfig[] {
    return configured === undefined ? DEFAULT_APPS : configured;
}

/** Keep the selected tab valid when settings arrive or the list is edited. */
export function resolveActiveServerAppId(apps: readonly ServerAppConfig[], activeId: string): string {
    return apps.some((app) => app.id === activeId) ? activeId : (apps[0]?.id ?? '');
}

// ── Branding CSS generator ────────────────────────────────────────────
// Pure CSS approach: hides third-party logos and uses :has() + ::before
// pseudo-elements to inject the company name text.  Everything flows
// through the proven `customCss` → <style> tag path — no JS or Rust
// changes required.
//
// :has() is supported in Chromium 105+ (WebView2 always tracks Edge stable).
// ::before content: "..." renders text in the logo's parent container,
// inheriting its layout slot.

/**
 * Build all branding + deletion CSS for a given app URL.
 * `companyName` is the white-label company name from identity.branding settings.
 */
function buildBrandingCss(url: string, companyName: string): string {
    const parts: string[] = [];
    // Escape quotes for safe CSS content: "..." value
    const safeName = companyName ? companyName.replace(/\\/g, '\\\\').replace(/"/g, '\\"') : '';

    // ── Immich (AI Gallery) ──────────────────────────────────────────
    if (url.includes(':2283')) {
        // Hide logo image (handle both src and alt matches)
        parts.push('img[src*="immich-logo"], img[alt*="Immich"] { display: none !important; }');

        // Show company name in logo's parent via ::before
        // Target specifically the link containing the logo. Use a robust selector.
        if (safeName) {
            // Ensure the parent is displayed as flex so the ::before content is visible and aligned.
            const parentSelector = 'a:has(> img[src*="immich-logo"]), a:has(> img[alt*="Immich"])';

            parts.push(`${parentSelector} { display: flex !important; align-items: center !important; text-decoration: none !important; }`);
            parts.push(`${parentSelector}::before { 
                content: "${safeName}" !important; 
                font-size: 1.3em; 
                font-weight: 700; 
                letter-spacing: .02em; 
                white-space: nowrap; 
                color: var(--immich-primary, inherit); /* Attempt to use primary color var if available */
            }`);
        }

        // Delete unwanted elements
        // KT: Tailwind utility-class selectors (.transition-all.py-4.rounded-2xl etc.)
        // are NOT safe to use for hiding — Immich reuses the same classes across
        // every page including Settings, so elements get hidden app-wide as you
        // navigate. Removed all Tailwind-combo selectors; only attribute/src
        // selectors (img[src*="..."]) are safe here.
    }

    // ── Syncthing (Syncing App) ───────────────────────────────────────
    if (url.includes(':8384')) {
        parts.push('img[src*="logo-horizontal"] { display: none !important; }');
        if (safeName) {
            parts.push(`*:has(> img[src*="logo-horizontal"])::before { content: "${safeName}" !important; font-size: 1.3em; font-weight: 700; letter-spacing: .02em; white-space: nowrap; display: inline-flex; align-items: center; }`);
        }
    }

    // ── Stirling PDF ─────────────────────────────────────────────────
    if (url.includes(':8080')) {
        parts.push('img[src*="StirlingPDFLogo"] { display: none !important; }');
        if (safeName) {
            parts.push(`*:has(> img[src*="StirlingPDFLogo"])::before { content: "${safeName}" !important; font-size: 1.3em; font-weight: 700; letter-spacing: .02em; white-space: nowrap; display: inline-flex; align-items: center; }`);
        }

        // Hide elements requested by user. 
        // Note: Previous generic selectors might have blocked main content, removing them.
        parts.push('.mantine-Paper-root.m_1b7284a3 { display: none !important; }');
        parts.push('.__m__-_r_iu_.mantine-Flex-root.m_8bffd616 { display: none !important; }');
        parts.push('.__m__-_r_0_.mantine-Flex-root.m_8bffd616 { display: none !important; }');
    }

    // ── Firewall (Adguard) ────────────────────────────────
    if (url.includes(':3000')) {
        parts.push('.header-brand-img { display: none !important; }');
        if (safeName) {
            parts.push(`*:has(> .header-brand-img)::before { content: "${safeName}" !important; font-size: 1.3em; font-weight: 700; letter-spacing: .02em; white-space: nowrap; display: inline-flex; align-items: center; }`);
        }

        // Hide footer and specific nav items as requested
        parts.push('.footer { display: none !important; }');
        parts.push('.footer__copyright { display: none !important; }');
        parts.push('.nav-link.order-4 { display: none !important; }');
    }

    return parts.join('\n');
}

export default function ServerAppsPanel() {
    const { data: settings } = useSettingsQuery();
    const patchSettings = usePatchSettings();

    // Defaults are only for a genuinely unconfigured install. An empty array
    // is a deliberate user configuration and must stay empty.
    const apps = resolveServerApps(settings?.ideal?.serverApps?.apps);

    // White-label company name — used to replace third-party logos with branded text
    const companyName = settings?.ideal?.identity?.branding?.companyName || '';

    const [activeAppId, setActiveAppId] = useState<string>(apps[0]?.id || '');
    const [manageOpen, setManageOpen] = useState(false);

    // Settings load asynchronously. The first render can use defaults while a
    // saved, completely different tab list is still arriving; reconcile the
    // selection so that transition cannot strand the panel with no webview.
    useEffect(() => {
        setActiveAppId((current) => resolveActiveServerAppId(apps, current));
    }, [apps]);

    const handleSaveApps = useCallback(async (updated: ServerAppConfig[]) => {
        await patchSettings.mutateAsync({ ideal: { serverApps: { apps: updated } } });
        // Switch to first tab if the current tab was removed
        if (updated.length && !updated.find(a => a.id === activeAppId)) {
            setActiveAppId(updated[0].id);
        }
    }, [patchSettings, activeAppId]);

    const activeApp = apps.find(a => a.id === activeAppId);

    const handleAppChange = useCallback((id: string) => {
        if (id === activeAppId) return;
        setActiveAppId(id);
    }, [activeAppId]);

    // Build merged CSS: branding CSS + app-specific custom CSS
    const mergedCss = activeApp
        ? [buildBrandingCss(activeApp.url, companyName), activeApp.customCss].filter(Boolean).join('\n') || null
        : null;

    if (!apps.length) {
        return (
            <div className="panel-container server-apps-panel">
                <NonIdealState
                    icon="applications"
                    title="No Server Apps Configured"
                    description="Add an app to open it here in a dedicated tab."
                    action={<Button onClick={() => setManageOpen(true)}>Manage Server Apps</Button>}
                />
                <ManageAppsDialog
                    isOpen={manageOpen}
                    onClose={() => setManageOpen(false)}
                    apps={apps}
                    onSave={handleSaveApps}
                />
            </div>
        );
    }

    return (
        <div className="panel-container server-apps-panel">
            <header className="mb-8 border-b border-[var(--color-border)] pb-4">
                <h1 className="text-3xl font-bold text-[var(--color-text-primary)] font-mono tracking-tight uppercase">
                    Server Apps
                </h1>
                <p className="text-[var(--color-text-secondary)] mt-2 font-mono text-sm">
                    Access self-hosted web applications.
                </p>
            </header>
            {/* Browser-style tab bar at top — spatially above the native webview */}
            <div className="server-tabs-bar" role="tablist" aria-label="Server applications">
                {apps.map((app) => (
                    <button
                        key={app.id}
                        type="button"
                        role="tab"
                        aria-selected={activeAppId === app.id}
                        className={`server-tab ${activeAppId === app.id ? 'active' : ''}`}
                        onClick={() => handleAppChange(app.id)}
                    >
                        <Icon icon={app.icon as IconName} size={14} />
                        <span className="server-tab-label">{app.name}</span>
                    </button>
                ))}

                {/* Manage apps button — pinned to the right of the tab bar */}
                <button
                    type="button"
                    className="server-tab server-tab-manage"
                    onClick={() => setManageOpen(true)}
                    title="Manage Server Apps"
                >
                    <Icon icon="settings" size={13} />
                    <span className="server-tab-label">Manage</span>
                </button>
            </div>

            <ManageAppsDialog
                isOpen={manageOpen}
                onClose={() => setManageOpen(false)}
                apps={apps}
                onSave={handleSaveApps}
            />

            {/* Universal EmbeddedWebView — handles open/resize/hide automatically */}
            {activeApp && (
                <EmbeddedWebView
                    key={activeApp.id}      // Force re-mount when tab changes
                    group="server-app"
                    id={activeApp.id}
                    url={activeApp.url}
                    customCss={mergedCss}
                    label={activeApp.name}
                />
            )}
        </div>
    );
}
