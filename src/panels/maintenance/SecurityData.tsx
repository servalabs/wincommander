import { useCallback, useEffect, useRef } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import {
  useBackend,
  type SecurityCveSnapshot,
  type SecurityThreatSnapshot,
} from "../../hooks/useBackend";
import { useMaintenanceSessionState } from "./maintenanceSessionState";

export function SecurityData() {
  const backend = useBackend();
  const backendRef = useRef(backend);
  backendRef.current = backend;
  const [threat, setThreat] = useMaintenanceSessionState<SecurityThreatSnapshot | undefined>("security-data.threat", undefined);
  const [cve, setCve] = useMaintenanceSessionState<SecurityCveSnapshot | undefined>("security-data.cve", undefined);
  const [loading, setLoading] = useMaintenanceSessionState("security-data.loading", false);
  const [hasLoaded, setHasLoaded] = useMaintenanceSessionState("security-data.has-loaded", false);
  const [threatError, setThreatError] = useMaintenanceSessionState<string | undefined>("security-data.threat-error", undefined);
  const [cveError, setCveError] = useMaintenanceSessionState<string | undefined>("security-data.cve-error", undefined);

  const refresh = useCallback(async () => {
    setLoading(true);
    setThreatError(undefined);
    setCveError(undefined);
    const [threatResult, cveResult] = await Promise.allSettled([
      backendRef.current.securityThreatSnapshot(),
      backendRef.current.securityCveSnapshot(),
    ]);
    if (threatResult.status === "fulfilled") {
      setThreat(threatResult.value);
    } else {
      setThreatError(securitySourceError("Local threat posture", threatResult.reason));
    }
    if (cveResult.status === "fulfilled") {
      setCve(cveResult.value);
    } else {
      setCveError(securitySourceError("Windows CVE coverage", cveResult.reason));
    }
    setLoading(false);
    setHasLoaded(true);
  }, [setCve, setCveError, setHasLoaded, setLoading, setThreat, setThreatError]);

  useEffect(() => {
    if (!hasLoaded && !loading) void refresh();
  }, [hasLoaded, loading, refresh]);

  const defenderTone = threat?.defender.status === "available" && threat.defender.realTimeEnabled
    ? "success" : "warning";
  const defenderStatus = threat?.defender.status ?? (loading ? "loading" : "unavailable");
  const realTimeProtection = threat
    ? threat.defender.status === "available"
      ? threat.defender.realTimeEnabled === true ? "Enabled" : "Disabled"
      : "Unavailable"
    : loading ? "Loading" : "Unavailable";
  const recentThreatCount = threat
    ? threat.defender.status === "available"
      ? String(threat.defender.recentThreatCount ?? "Unavailable")
      : "Unavailable"
    : loading ? "Loading" : "Unavailable";
  const cveStatusLabel = cveError
    ? "Unavailable"
    : cve?.status === "ok" ? "Available" : loading ? "Loading" : "Provider required";

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader>
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div><CardTitle>Security data</CardTitle><CardDescription>Local posture only. No file paths, network endpoints, account addresses, or threat names are displayed.</CardDescription></div>
            <Button size="icon" variant="outline" disabled={loading} onClick={() => void refresh()} title={hasLoaded ? "Refresh security data" : "Scan security data"} aria-label={hasLoaded ? "Refresh security data" : "Scan security data"}><Icon icon={hasLoaded ? "refresh" : "search"} className={loading ? "animate-spin" : undefined} /></Button>
          </div>
        </CardHeader>
        {loading && <CardContent><p className="text-sm text-[var(--text-dim)]" role="status" aria-live="polite">Collecting local security posture and Windows CVE coverage…</p></CardContent>}
        {(threatError || cveError) && <CardContent><div className="space-y-1 text-sm text-[var(--danger)]" role="alert">
          {threatError && <p>{threatError}</p>}
          {cveError && <p>{cveError}</p>}
        </div></CardContent>}
      </Card>

      <Card>
        <CardHeader>
          <div className="flex items-center justify-between gap-2"><CardTitle>Local threat posture</CardTitle><Badge tone={defenderTone}>{defenderStatus}</Badge></div>
          <CardDescription>Microsoft Defender summary plus aggregate adapter activity from this device.</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-3 sm:grid-cols-3" aria-busy={loading}>
          <Metric label="Real-time protection" value={realTimeProtection} />
          <Metric label="Recent detections" value={recentThreatCount} />
          <Metric label="Active adapters" value={threat ? `${threat.network.activeInterfaceCount} / ${threat.network.interfaceCount}` : loading ? "Loading" : "Unavailable"} />
          {Object.entries(threat?.defender.severityCounts ?? {}).map(([severity, count]) => <Metric key={severity} label={`${severity} detections`} value={String(count)} />)}
          {threat?.defender.status === "unavailable" && <p className="text-sm text-[var(--text-dim)] sm:col-span-3" role="status">Defender could not be queried. Unavailable counts do not mean zero detections or a clean scan.</p>}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div className="flex items-center justify-between gap-2"><CardTitle>Windows CVE coverage</CardTitle><Badge tone={cve?.status === "ok" ? "success" : "warning"}>{cveStatusLabel}</Badge></div>
          <CardDescription>{cveError ? "The configured provider did not return a Windows coverage result." : cve?.status === "ok" ? `${cve.results.length} bounded result${cve.results.length === 1 ? "" : "s"} for Windows ${cve.queriedVersion}.` : "OSV is pinned for package-version data, but it does not map Windows OS versions. An approved Windows provider is required."}</CardDescription>
        </CardHeader>
        <CardContent><p className="font-mono text-xs text-[var(--text-mute)]">Source: {cve?.source ?? "osv"} · updated: {cve?.sourceTimestamp ?? "—"}</p>{cve?.status !== "ok" && !loading && !cveError && <p className="mt-2 text-sm text-[var(--text-dim)]" role="status">No Windows-specific coverage is available from the configured provider yet.</p>}</CardContent>
      </Card>
    </div>
  );
}

function securitySourceError(source: string, cause: unknown): string {
  const message = cause instanceof Error ? cause.message : String(cause);
  if (/pro[- ]feature failed/i.test(message)) {
    return `${source} could not be loaded from WinCommander Pro. Confirm that Pro is running, then refresh.`;
  }
  if (/timed? ?out|timeout/i.test(message)) {
    return `${source} could not be loaded because the request timed out. Try again later.`;
  }
  if (/provider unavailable|provider rejected|provider response failed/i.test(message)) {
    return `${source} could not be loaded from its provider. Try again later.`;
  }
  if (/decoy mode/i.test(message)) {
    return `${source} is unavailable in Decoy mode.`;
  }
  return `${source} could not be loaded. Try refreshing.`;
}

function Metric({ label, value }: { label: string; value: string }) {
  return <div className="rounded-[var(--r)] border border-[var(--border)] p-3"><p className="text-xs text-[var(--text-mute)]">{label}</p><p className="mt-1 font-mono text-sm text-[var(--text)]">{value}</p></div>;
}
