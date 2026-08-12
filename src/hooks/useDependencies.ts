// ════════════════════════════════════════════════════════════════════════════
// useDependencies — Centralized dependency management hook
// ════════════════════════════════════════════════════════════════════════════
// Wraps the new backend dependency module commands.
// Provides typed access to check, install, and manage dependencies.
// ════════════════════════════════════════════════════════════════════════════

import { useCallback, useMemo } from 'react';
import { executeBackendCommand } from './useBackend';
import type { PanelId } from '../types/panels';

export interface DependencyInfo {
    id: string;
    name: string;
    panelId: PanelId;
    installed: boolean;
    version: string | null;
    running: boolean | null;
    connected: boolean | null;
    missing: string[] | null;
    canHide: boolean;
    canStart: boolean;
}

export interface DependencyStatusResponse {
    dependencies: DependencyInfo[];
    cacheAgeSecs?: number;  // 0 = fresh probe; >0 = served from file cache
}

export interface InstallDependencyResponse {
    success?: boolean;
    error?: boolean;
    id: string;
    message: string;
    hidden?: number;
    started?: boolean;
}

export default function useDependencies() {
    const getDependencyStatus = useCallback(
        (force = false) => executeBackendCommand<DependencyStatusResponse>(
            "Get-DependencyStatus",
            force ? { Force: true } : {}
        ),
        []
    );

    const installDependency = useCallback(
        (id: string, target?: string) =>
            executeBackendCommand<InstallDependencyResponse>("Install-Dependency", {
                Id: id,
                ...(target ? { Target: target } : {}),
            }),
        []
    );

    return useMemo(
        () => ({
            getDependencyStatus,
            installDependency,
        }),
        [getDependencyStatus, installDependency]
    );
}
