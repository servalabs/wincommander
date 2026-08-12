import { useCallback, useEffect, useState } from 'react';
import { Button, ProgressBar, Icon, Callout, Collapse } from "@/components/ui/bp";
import useBackend from '../../hooks/useBackend';
import { showError } from '../../utils/toast';

interface Dependency {
    name: string;
    status: 'installed' | 'missing';
    path?: string;
}

const FRIENDLY_NAMES: Record<string, string> = {
    "Python": "Python Runtime",
    "mediapipe": "ML Engine",
    "opencv-python": "Vision Core",
    "PyQt6": "UI Framework",
    "numpy": "Math Library",
    "Pillow": "Image Processing"
};

const cleanInstallError = (value: unknown): string => {
    const raw = value instanceof Error ? value.message : String(value || "Installation failed");
    const jsonStart = raw.indexOf('{"error"');
    if (jsonStart >= 0) {
        try {
            const parsed = JSON.parse(raw.slice(jsonStart).trim());
            if (parsed?.message) return cleanInstallError(parsed.message);
        } catch {
            // Fall through to pattern cleanup below.
        }
    }

    if (/verification failed for mediapipe/i.test(raw)) {
        return "ML Engine could not be verified after installation. Retry will run the VC++ runtime and NumPy compatibility repair before checking again.";
    }

    const firstLine = raw
        .replace(/^Command failed \(\d+\):\s*/i, "")
        .split(/\r?\n/)
        .map(line => line.trim())
        .find(line => line && !line.startsWith("{") && !line.startsWith("At line:"));

    return firstLine || "Installation failed. Please retry.";
};

interface AIRuntimeInstallerProps {
    onInstalled: () => void;
}

export default function AIRuntimeInstaller({ onInstalled }: AIRuntimeInstallerProps) {
    const { getAIDependenciesStatus, installAIDependencies } = useBackend();
    const [dependencies, setDependencies] = useState<Dependency[]>([]);
    const [loading, setLoading] = useState(true);
    const [installing, setInstalling] = useState(false);
    const [currentInstalling, setCurrentInstalling] = useState<string | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [detailsOpen, setDetailsOpen] = useState(false);

    const fetchStatus = useCallback(async () => {
        setLoading(true);
        setError(null);
        try {
            const res = await getAIDependenciesStatus();
            if (res.success && res.data && res.data.details) {
                const mappedDeps: Dependency[] = res.data.details.map((d: any) => ({
                    name: d.name,
                    status: d.status === 'installed' ? 'installed' : 'missing',
                    path: d.path
                }));
                setDependencies(mappedDeps);

                const missing = mappedDeps.filter(d => d.status === 'missing');
                if (missing.length === 0) {
                    onInstalled();
                    return true;
                }
                return false;
            } else {
                const errString = res.error || "Failed to fetch dependency status";
                setError(errString);
                showError(errString);
            }
        } catch (e) {
            const errString = e instanceof Error ? e.message : "An unknown error occurred";
            setError(errString);
            showError(errString);
        } finally {
            setLoading(false);
        }
        return false;
    }, [getAIDependenciesStatus, onInstalled]);

    useEffect(() => {
        fetchStatus();
    }, [fetchStatus]);

    const handleInstall = async () => {
        setInstalling(true);
        setError(null);
        setDetailsOpen(true); // Auto-open details to show progress

        const missing = dependencies.filter(d => d.status === 'missing');

        for (const dep of missing) {
            setCurrentInstalling(dep.name);
            try {
                const res = await installAIDependencies(dep.name);
                if (!res.success) {
                    throw new Error(res.error || `Failed to install ${dep.name}`);
                }

                setDependencies(prev => prev.map(d =>
                    d.name === dep.name ? { ...d, status: 'installed' } : d
                ));
            } catch (e) {
                const errString = cleanInstallError(e);
                setError(errString);
                showError(errString);
                setInstalling(false);
                setCurrentInstalling(null);
                return;
            }
        }

        // Post-install verification can race with PATH refresh / pip cache. Retry a few
        // times before declaring failure so users don't see a spurious error.
        let allInstalled = await fetchStatus();
        for (let attempt = 0; !allInstalled && attempt < 3; attempt++) {
            await new Promise(r => setTimeout(r, 1500));
            allInstalled = await fetchStatus();
        }

        setInstalling(false);
        setCurrentInstalling(null);

        if (!allInstalled) {
            const err = "Some components failed to verify after installation. Please retry.";
            setError(err);
            showError(err);
        }
    };

    if (loading) return null; // Don't show anything while initial check

    const missingCount = dependencies.filter(d => d.status === 'missing').length;
    if (missingCount === 0 && !installing) {
        // Double check onInstalled call to ensure we hide if missed
        setTimeout(onInstalled, 0);
        return null;
    }

    return (
        <div className="flex flex-col gap-2 p-3 bg-[var(--bg-subtle)] rounded-md border border-[var(--border-subtle)] shadow-sm animate-in fade-in slide-in-from-top-1 duration-300">
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                    <div className="flex items-center justify-center w-8 h-8 rounded-full bg-[var(--color-warning)]/10 text-[var(--color-warning)]">
                        <Icon icon="warning-sign" size={14} />
                    </div>
                    <div>
                        <div className="text-sm font-semibold text-[var(--text-primary)]">
                            AI Runtime Incomplete
                        </div>
                        <div className="text-xs text-[var(--text-muted)]">
                            {missingCount} component{missingCount !== 1 ? 's' : ''} missing
                        </div>
                    </div>
                </div>
                <div className="flex items-center gap-2">
                    <Button
                        minimal
                        small
                        text={detailsOpen ? "Hide Details" : "Details"}
                        rightIcon={detailsOpen ? "chevron-up" : "chevron-down"}
                        onClick={() => setDetailsOpen(!detailsOpen)}
                        disabled={installing}
                    />
                    <Button
                        intent="primary"
                        small
                        loading={installing}
                        text={installing ? "Installing..." : "Install All"}
                        icon="download"
                        onClick={handleInstall}
                    />
                </div>
            </div>

            {error && (
                <div role="alert">
                    <Callout intent="danger" title="Installation Error" icon={null} className="mt-2 text-xs">
                        {error}
                        <Button minimal small icon="refresh" onClick={fetchStatus} className="ml-2" text="Retry" />
                    </Callout>
                </div>
            )}

            <Collapse isOpen={detailsOpen}>
                <div className="mt-2 pt-2 border-t border-[var(--border-subtle)] grid grid-cols-1 sm:grid-cols-2 gap-x-4 gap-y-1">
                    {dependencies.map((dep) => (
                        <div key={dep.name} className="flex items-center justify-between text-xs py-1 px-1 hover:bg-[var(--bg-hover)] rounded transition-colors">
                            <span className={`font-medium ${dep.status === 'installed' ? 'text-[var(--text-muted)]' : 'text-[var(--text-primary)]'}`}>
                                {FRIENDLY_NAMES[dep.name] || dep.name}
                            </span>
                            <div className="flex items-center gap-2 min-h-[20px]">
                                {currentInstalling === dep.name ? (
                                    <div className="flex items-center gap-2">
                                        <div className="w-8"><ProgressBar intent="primary" animate className="h-1" /></div>
                                    </div>
                                ) : dep.status === 'installed' ? (
                                    <span className="flex items-center text-[var(--color-success)] opacity-75">
                                        <Icon icon="tick" size={12} />
                                    </span>
                                ) : (
                                    <span className="flex items-center text-[var(--color-warning)]">
                                        <Icon icon="cross" size={12} />
                                    </span>
                                )}
                            </div>
                        </div>
                    ))}
                </div>
            </Collapse>
        </div>
    );
}
