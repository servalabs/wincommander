import { useState, useCallback, useMemo } from "react";
import { Button, Icon, Spinner } from "@/components/ui/bp";
import { cn } from "../../../lib/utils";
import { executeBackendCommand } from "../../../hooks/useBackend";
import { useAppState } from "../../../context/AppContext";
import { showSuccess, showError } from "../../../utils/toast";
import type { DependencyInfo } from "../../../hooks/useDependencies";

const CRITICAL_ENGINES = new Set([
  "ramDiskEngine", "instantSearch",
  "diskHealthEngine", "systemCleaner", "winget", "powershell7",
]);
const OPTIONAL_ENGINES = new Set([
  "meshVpn", "productivityEngine", "privacyShieldAI",
  "metadataScrubber", "localLlm", "vcredist", "chocolatey", "scoop",
]);

const ENGINE_DESCRIPTIONS: Record<string, string> = {
  ramDiskEngine:       "RAM-backed temp disk for fast ephemeral ops",
  instantSearch:       "Lightning-fast file-search indexing",
  diskHealthEngine:    "S.M.A.R.T. disk health monitoring",
  systemCleaner:       "Deep junk removal beyond Disk Cleanup",
  winget:              "Windows Package Manager for app installs",
  powershell7:         "Modern PowerShell runtime for all scripts",
  chocolatey:          "Community package manager for app installs & updates",
  scoop:               "Command-line installer for portable apps & tools",
  meshVpn:             "Peer-to-peer encrypted mesh networking",
  productivityEngine:  "Virtual desktop & focus-mode features",
  privacyShieldAI:     "AI-powered behaviour analysis shield",
  metadataScrubber:    "Strip metadata from files before sharing",
  localLlm:            "On-device AI inference engine",
  vcredist:            "Visual C++ runtime libraries",
};

// Blueprint glyph per engine so the cards carry a real icon tile like the app
// cards do, instead of the old bare colour dot. Unknown ids fall back to "cog".
const ENGINE_ICONS: Record<string, string> = {
  ramDiskEngine:      "database",
  instantSearch:      "search",
  diskHealthEngine:   "pulse",
  systemCleaner:      "eraser",
  winget:             "cube",
  powershell7:        "console",
  chocolatey:         "shopping-cart",
  scoop:              "archive",
  meshVpn:            "globe-network",
  productivityEngine: "desktop",
  privacyShieldAI:    "shield",
  metadataScrubber:   "clean",
  localLlm:           "predictive-analysis",
  vcredist:           "code",
};

function engineImportance(id: string): "critical" | "optional" | "system" {
  if (CRITICAL_ENGINES.has(id)) return "critical";
  if (OPTIONAL_ENGINES.has(id)) return "optional";
  return "system";
}

// Engine version strings carry build metadata the user shouldn't see: .NET
// embeds "<semver>+<git-sha>", pwsh embeds "<semver> SHA: <hash>", and
// git-describe builds append "-g<hash>"/"-t<hash>". Show only the clean version.
function formatEngineVersion(version: string): string {
  return version
    .split(/\s+SHA/i)[0]
    .split("+")[0]
    .replace(/-[gt][0-9a-f]{7,}.*$/i, "")
    .trim();
}

interface CardProps {
  dep: DependencyInfo;
  importance: "critical" | "optional" | "system";
  isBusy: boolean;
  onInstall: (dep: DependencyInfo) => void;
}

// Engine cards reuse the app-card markup/classes (see AppInstallerPanel.css) so
// they read as the same component family as the app grid — real icon tile,
// wrapping name (no ellipsis clipping), 2-line description, right-aligned
// action — instead of the old bespoke .eng-card layout that clipped names.
function EngineCard({ dep, importance, isBusy, onInstall }: CardProps) {
  const version = dep.version ? formatEngineVersion(dep.version) : null;
  return (
    <div className={cn("app-card", "eng-app-card", dep.installed && "installed")}>
      <span className={cn("app-icon-fallback", "eng-app-icon", `eng-app-icon--${importance}`)}>
        <Icon icon={ENGINE_ICONS[dep.id] ?? "cog"} size={16} />
      </span>
      <div className="app-info">
        <span className="app-name">
          {dep.name}
          {dep.installed && (
            <Icon
              icon="tick"
              size={12}
              className="installed-badge"
              title={dep.running ? "Running" : "Installed"}
            />
          )}
        </span>
        {ENGINE_DESCRIPTIONS[dep.id] && (
          <span className="app-description">{ENGINE_DESCRIPTIONS[dep.id]}</span>
        )}
        {dep.installed && version && <span className="app-version mono">{version}</span>}
      </div>
      {dep.installed ? (
        <span className={cn("eng-status-chip", dep.running ? "eng-status-chip--live" : "eng-status-chip--ready")}>
          {dep.running ? "Live" : "Ready"}
        </span>
      ) : (
        <Button
          small
          minimal
          intent={importance === "critical" ? "danger" : "success"}
          icon={isBusy ? undefined : "download"}
          loading={isBusy}
          disabled={isBusy}
          onClick={() => onInstall(dep)}
        >
          {isBusy ? "" : "Install"}
        </Button>
      )}
    </div>
  );
}

// Packages & Apps is itself built on winget — listing "Package Manager" as
// just another optional engine card here reads as redundant/circular, so it's
// excluded from this grid. It's still a real dependency elsewhere (DependencyGate,
// Install-Dependency, etc.) — only this display is filtered.
const HIDDEN_FROM_ENGINES_GRID = new Set([
  "winget",
  // Chocolatey and Scoop remain available as deliberate, manual choices in
  // Package updates. They are never part of the engine readiness surface.
  "chocolatey",
  "scoop",
]);

export default function EnginesSection() {
  const { dependencyStatus: allDependencyStatus, forceRefreshDeps, runAppInventoryScan } = useAppState();
  const dependencyStatus = useMemo(
    () => allDependencyStatus?.filter((d) => !HIDDEN_FROM_ENGINES_GRID.has(d.id)) ?? null,
    [allDependencyStatus]
  );
  const [installingIds, setInstallingIds] = useState<Set<string>>(new Set());
  const [installingAll, setInstallingAll] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [showInstalled, setShowInstalled] = useState(false);

  const handleInstall = useCallback(async (dep: DependencyInfo) => {
    setInstallingIds((prev) => new Set([...prev, dep.id]));
    try {
      const r = await executeBackendCommand<any>("Install-Dependency", { Id: dep.id });
      if (r.success) {
        showSuccess(`${dep.name} installed.`);
        await Promise.all([forceRefreshDeps(), runAppInventoryScan(true)]);
      } else {
        showError(r.error || `Failed to install ${dep.name}.`);
      }
    } finally {
      setInstallingIds((prev) => { const n = new Set(prev); n.delete(dep.id); return n; });
    }
  }, [forceRefreshDeps, runAppInventoryScan]);

  const handleInstallAll = useCallback(async () => {
    if (!dependencyStatus) return;
    const toInstall = dependencyStatus.filter((d) => !d.installed);
    const toStart = dependencyStatus.filter((d) => d.installed && d.canStart && d.running === false);
    const targets = [...toInstall, ...toStart];
    if (targets.length === 0) return;

    setInstallingAll(true);
    setInstallingIds((prev) => new Set([...prev, ...targets.map((d) => d.id)]));
    try {
      // KT: each dependency is its own independent backend call, run in
      // parallel — so one slow/hung dependency (Privacy Shield's pip
      // packages, a stalled winget download) can't block the others.
      // "Install all" used to run every dependency sequentially inside ONE
      // PowerShell process via Install-AllDependencies, so a single wedged
      // dependency stalled the whole batch, including fast ones queued
      // after it.
      const outcomes = await Promise.all(targets.map((dep) =>
        executeBackendCommand<any>(
          dep.installed ? "Start-DependencyService" : "Install-Dependency",
          { Id: dep.id }
        ).then((r) => ({ dep, r }))
      ));

      const failures = outcomes.filter((o) => !o.r.success).map((o) => o.r.error || o.dep.name);
      if (failures.length === 0) {
        showSuccess(`${targets.length} engine${targets.length === 1 ? "" : "s"} ready.`);
      } else {
        showError(`${failures.length} of ${targets.length} engines failed: ${failures.join(", ")}`);
      }
      await Promise.all([forceRefreshDeps(), runAppInventoryScan(true)]);
    } finally {
      setInstallingAll(false);
      setInstallingIds((prev) => {
        const n = new Set(prev);
        targets.forEach((d) => n.delete(d.id));
        return n;
      });
    }
  }, [dependencyStatus, forceRefreshDeps, runAppInventoryScan]);

  const handleRefresh = useCallback(async () => {
    setRefreshing(true);
    try { await forceRefreshDeps(); }
    finally { setRefreshing(false); }
  }, [forceRefreshDeps]);

  if (!dependencyStatus) {
    return <div className="eng-loading"><Spinner size={16} /></div>;
  }

  const missing = dependencyStatus.filter((d) => !d.installed);
  const installed = dependencyStatus.filter((d) => d.installed);
  const critical = missing.filter((d) => engineImportance(d.id) === "critical");
  const optional = missing.filter((d) => engineImportance(d.id) !== "critical");
  const pct = dependencyStatus.length > 0
    ? Math.round((installed.length / dependencyStatus.length) * 100)
    : 100;

  return (
    <div className="eng-root">
      {/* Header */}
      <div className="eng-header">
        <div className="eng-health">
          <div className="eng-health-track">
            <div className="eng-health-fill" style={{ width: `${pct}%` }} />
          </div>
          <span className="eng-health-label">
            {installed.length}/{dependencyStatus.length} ready
          </span>
        </div>
        <div className="eng-header-actions">
          {missing.length > 1 && (
            <Button
              small minimal intent="primary"
              icon="download" loading={installingAll}
              disabled={installingAll} onClick={handleInstallAll}
            >
              Install all
            </Button>
          )}
          <Button
            small minimal icon="refresh"
            loading={refreshing} onClick={handleRefresh}
            title="Re-detect installed engines"
          />
        </div>
      </div>

      {/* Critical missing */}
      {critical.length > 0 && (
        <div className="eng-group">
          <div className="eng-group-label eng-group-label--critical">
            <Icon icon="error" size={10} /> Critical
          </div>
          <div className="eng-app-grid">
            {critical.map((dep) => (
              <EngineCard
                key={dep.id} dep={dep}
                importance="critical"
                isBusy={installingIds.has(dep.id) || installingAll}
                onInstall={handleInstall}
              />
            ))}
          </div>
        </div>
      )}

      {/* Optional / system missing */}
      {optional.length > 0 && (
        <div className="eng-group">
          <div className="eng-group-label eng-group-label--optional">
            <Icon icon="warning-sign" size={10} /> Optional
          </div>
          <div className="eng-app-grid">
            {optional.map((dep) => (
              <EngineCard
                key={dep.id} dep={dep}
                importance={engineImportance(dep.id)}
                isBusy={installingIds.has(dep.id) || installingAll}
                onInstall={handleInstall}
              />
            ))}
          </div>
        </div>
      )}

      {/* All-ok banner */}
      {missing.length === 0 && (
        <div className="eng-all-ok">
          <Icon icon="tick-circle" size={12} />
          <span>All engines ready</span>
        </div>
      )}

      {/* Installed collapsible */}
      {installed.length > 0 && (
        <div className="eng-installed">
          <button
            className="eng-installed-toggle"
            onClick={() => setShowInstalled((v) => !v)}
          >
            <Icon icon={showInstalled ? "chevron-down" : "chevron-right"} size={10} />
            <span>Installed ({installed.length})</span>
          </button>
          {showInstalled && (
            <div className="eng-app-grid">
              {installed.map((dep) => (
                <EngineCard
                  key={dep.id} dep={dep}
                  importance={engineImportance(dep.id)}
                  isBusy={installingIds.has(dep.id)}
                  onInstall={handleInstall}
                />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
