// ════════════════════════════════════════════════════════════════════════════
// DependencyGate — Full-screen overlay shown when a panel's dependency is
// missing or not yet running. Renders an install/start prompt instead of
// the panel content.
// ════════════════════════════════════════════════════════════════════════════

import React, { useState, useCallback, useEffect } from 'react';
import { motion } from 'framer-motion';
import { CompatButton as Button } from '@/components/ui/compat-button';
import { Icon } from '@/components/ui/icon';
import { useAppState } from '../context/AppContext';
import { executeBackendCommand } from '../hooks/useBackend';
import { runOperation } from '../context/OperationContext';
import type { PanelId } from '../types/panels';
import { DURATION_S, EASE } from './shared/motion';
import './DependencyGate.css';

// ── Gate card entrance ─────────────────────────────────────────────────────
// Matches the overlay pattern: fade + slight scale-in from 0.96.
// <MotionConfig> in App.tsx handles reduced-motion globally.
const gateCardVariants = {
    hidden:  { opacity: 0, scale: 0.96 },
    visible: { opacity: 1, scale: 1 },
} as const;

const gateCardTransition = {
    duration: DURATION_S.slow, // 300 ms — panel-weight feel
    ease: EASE.enter,
} as const;

/** Fallback panel → dependency ID mapping used when Get-DependencyStatus hasn't resolved yet */
const PANEL_DEP_ID: Partial<Record<string, string>> = {
    productivity:   'productivityEngine',
    'search-files': 'instantSearch',
    'private-mesh': 'meshVpn',
    apps:           'winget',
    advisor:        'localLlm',
};

/** Fallback panel → dependency display name */
const PANEL_DEP_NAME: Partial<Record<string, string>> = {
    productivity:   'Productivity Engine',
    'search-files': 'Instant Search Engine',
    'private-mesh': 'Private Mesh VPN',
    apps:           'Package Manager',
    advisor:        'Local AI Advisor',
};

/** Panel-specific feature previews shown in the install gate */
const FEATURE_PREVIEWS: Record<string, { tagline: string; features: string[] }> = {
    productivity: {
        tagline: 'See how you spend time on your computer.',
        features: [
            'Which apps you use and for how long',
            'Deep work vs. distraction patterns',
            'Hour-by-hour activity timeline',
        ],
    },
};

interface DependencyGateProps {
    panelId: PanelId;
    children: React.ReactNode;
}

function formatCacheAge(secs: number | null): string {
    if (secs === null || secs === 0) return 'just now';
    if (secs < 3600) return `${Math.floor(secs / 60)} min ago`;
    if (secs < 86400) return `${Math.floor(secs / 3600)} hr ago`;
    return `${Math.floor(secs / 86400)} days ago`;
}

export default function DependencyGate({ panelId, children }: DependencyGateProps) {
    const { dependencyStatus, depCacheAge, forceRefreshDeps } = useAppState();
    const [installing, setInstalling] = useState(false);
    const [starting, setStarting] = useState(false);
    const [refreshing, setRefreshing] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [startError, setStartError] = useState<string | null>(null);
    const [installDone, setInstallDone] = useState(false);
    // Incremented to drive the post-install retry loop; reset on each install attempt.
    const [verifyAttempt, setVerifyAttempt] = useState(0);

    const dep     = dependencyStatus?.find(d => d.panelId === panelId) ?? null;
    const depId   = dep?.id   ?? PANEL_DEP_ID[panelId]   ?? null;
    const depName = dep?.name ?? PANEL_DEP_NAME[panelId] ?? 'Required Software';

    const handleForceRefresh = useCallback(async () => {
        setRefreshing(true);
        try { await forceRefreshDeps(); } finally { setRefreshing(false); }
    }, [forceRefreshDeps]);

    const handleInstall = useCallback(async () => {
        if (!depId) return;
        setInstalling(true);
        setError(null);
        setInstallDone(false);
        setVerifyAttempt(0);
        try {
            await runOperation(`Install ${depName}`, [
                {
                    label: `Installing ${depName}`,
                    fn: async () => {
                        const res = await executeBackendCommand('Install-Dependency', { Id: depId });
                        if (!res.success) throw new Error(res.error || 'Installation failed');
                        if (res.data && (res.data as any).error) throw new Error((res.data as any).message || 'Installation failed');
                    },
                },
                {
                    // Force-bypass both the 15s in-memory and 12hr file caches so
                    // the gate clears immediately after a successful winget install.
                    label: 'Refreshing status',
                    fn: async () => { await forceRefreshDeps(); },
                },
            ], { mode: 'sequential', accent: 'blue', failFast: true, autoDismissMs: 4000 });
            setInstallDone(true);
        } catch (err) {
            setError(err instanceof Error ? err.message : String(err));
        } finally {
            setInstalling(false);
        }
    }, [depId, depName, forceRefreshDeps]);

    // Post-install retry: if the first forced probe still returns not-installed
    // (winget PATH-refresh lag), retry up to 3× at 2s intervals automatically.
    useEffect(() => {
        if (!installDone || dep?.installed || verifyAttempt >= 3) return;
        const timer = setTimeout(async () => {
            await forceRefreshDeps();
            setVerifyAttempt(v => v + 1);
        }, 2000);
        return () => clearTimeout(timer);
    }, [installDone, dep?.installed, verifyAttempt, forceRefreshDeps]);

    const handleStart = useCallback(async () => {
        if (!depId) return;
        setStarting(true);
        setStartError(null);
        try {
            await runOperation(`Start ${depName}`, [
                {
                    label: `Starting ${depName}`,
                    fn: async () => {
                        const res = await executeBackendCommand('Start-DependencyService', { Id: depId });
                        if (!res.success) throw new Error(res.error || 'Failed to start service');
                        if (res.data && (res.data as any).error) throw new Error((res.data as any).message || 'Failed to start service');
                    },
                },
                {
                    label: 'Refreshing status',
                    fn: async () => { await forceRefreshDeps(); },
                },
            ], { mode: 'sequential', accent: 'blue', failFast: true, autoDismissMs: 3000 });
        } catch (err) {
            setStartError(err instanceof Error ? err.message : String(err));
        } finally {
            setStarting(false);
        }
    }, [depId, depName, forceRefreshDeps]);

    // KT: While the dep probe is still in flight, render the panel optimistically
    // instead of flashing a "Checking for <engine>…" card for ~500 ms on startup.
    // If the dep turns out to be missing, the install gate below will appear
    // once Get-DependencyStatus resolves. Panels render their own internal
    // "not installed" placeholders too (e.g. vault), so an optimistic render
    // is safe — worst case the user sees the install gate one tick later.
    if (dependencyStatus === null || !dep || (dep.installed && (!dep.canStart || dep.running !== false))) {
        return <>{children}</>;
    }

    // Running gate: engine installed but background service not yet up.
    if (dep.installed && dep.canStart && dep.running === false) {
        return (
            <div className="dependency-gate">
                <motion.div
                    className="dependency-gate__card"
                    variants={gateCardVariants}
                    initial="hidden"
                    animate="visible"
                    transition={gateCardTransition}
                >
                    <Icon icon="play" size={48} className="dependency-gate__icon" />
                    <h2 className="dependency-gate__title">{depName} Not Running</h2>
                    <p className="dependency-gate__desc">
                        <strong>{depName}</strong> is installed but the background service
                        is not currently running.
                    </p>

                    {startError && (
                        <div className="dependency-gate__error">
                            <Icon icon="warning-sign" size={14} />
                            <span>{startError}</span>
                        </div>
                    )}

                    <div className="dependency-gate__cache-row">
                        <span className="dependency-gate__cache-age">
                            Last checked {formatCacheAge(depCacheAge)}
                        </span>
                        <Button
                            minimal
                            small
                            icon={refreshing ? undefined : 'refresh'}
                            loading={refreshing}
                            disabled={refreshing || starting}
                            onClick={handleForceRefresh}
                            className="dependency-gate__refresh-btn"
                        >
                            {refreshing ? '' : 'Refresh'}
                        </Button>
                    </div>

                    {depId && (
                        <Button
                            intent="primary"
                            size="large"
                            icon={starting ? undefined : 'play'}
                            loading={starting}
                            onClick={handleStart}
                            disabled={starting || refreshing}
                            className="dependency-gate__btn"
                        >
                            {starting ? 'Starting…' : `Start ${depName}`}
                        </Button>
                    )}
                </motion.div>
            </div>
        );
    }

    // Install gate: engine not yet installed.
    const preview = FEATURE_PREVIEWS[panelId];

    return (
        <div className="dependency-gate">
            <motion.div
                className="dependency-gate__card"
                variants={gateCardVariants}
                initial="hidden"
                animate="visible"
                transition={gateCardTransition}
            >
                <Icon icon="download" size={48} className="dependency-gate__icon" />
                <h2 className="dependency-gate__title">{depName} Required</h2>

                {preview ? (
                    <>
                        <p className="dependency-gate__tagline">{preview.tagline}</p>
                        <ul className="dependency-gate__features">
                            {preview.features.map((f, i) => (
                                <li key={i}>{f}</li>
                            ))}
                        </ul>
                    </>
                ) : (
                    <p className="dependency-gate__desc">
                        This section requires <strong>{depName}</strong> to function.
                        Click below to install it automatically.
                    </p>
                )}

                {error && (
                    <div className="dependency-gate__error">
                        <Icon icon="warning-sign" size={14} />
                        <span>{error}</span>
                    </div>
                )}

                {installDone && !error && (
                    <div className="dependency-gate__success">
                        <Icon icon="tick-circle" size={14} />
                        <span>Installation completed. Please restart the app if this screen persists.</span>
                    </div>
                )}

                <div className="dependency-gate__cache-row">
                    <span className="dependency-gate__cache-age">
                        Last checked {formatCacheAge(depCacheAge)}
                    </span>
                    <Button
                        minimal
                        small
                        icon={refreshing ? undefined : 'refresh'}
                        loading={refreshing}
                        disabled={refreshing || installing}
                        onClick={handleForceRefresh}
                        className="dependency-gate__refresh-btn"
                    >
                        {refreshing ? '' : 'Refresh'}
                    </Button>
                </div>

                {depId && (
                    <Button
                        intent="primary"
                        size="large"
                        icon={installing ? undefined : 'download'}
                        loading={installing}
                        onClick={handleInstall}
                        disabled={installing || refreshing}
                        className="dependency-gate__btn"
                    >
                        {installing ? 'Installing…' : `Install ${depName}`}
                    </Button>
                )}
            </motion.div>
        </div>
    );
}
