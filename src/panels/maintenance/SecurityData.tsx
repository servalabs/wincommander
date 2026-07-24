import { useCallback, useEffect, useRef } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import {
  useBackend,
  type SecurityBreachMonitorStatus,
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
  const [breach, setBreach] = useMaintenanceSessionState<SecurityBreachMonitorStatus | undefined>("security-data.breach", undefined);
  const [optIn, setOptIn] = useMaintenanceSessionState("security-data.opt-in", false);
  const [loading, setLoading] = useMaintenanceSessionState("security-data.loading", false);
  const [hasLoaded, setHasLoaded] = useMaintenanceSessionState("security-data.has-loaded", false);
  const [error, setError] = useMaintenanceSessionState<string | undefined>("security-data.error", undefined);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(undefined);
    try {
      const [nextThreat, nextCve, nextBreach] = await Promise.all([
        backendRef.current.securityThreatSnapshot(),
        backendRef.current.securityCveSnapshot(),
        backendRef.current.securityBreachMonitorStatus(optIn),
      ]);
      setThreat(nextThreat);
      setCve(nextCve);
      setBreach(nextBreach);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setLoading(false);
      setHasLoaded(true);
    }
  }, [optIn, setBreach, setCve, setError, setHasLoaded, setLoading, setThreat]);

  useEffect(() => {
    if (!hasLoaded && !loading) void refresh();
  }, [hasLoaded, loading, refresh]);

  const updateOptIn = useCallback(async (nextOptIn: boolean) => {
    setOptIn(nextOptIn);
    setLoading(true);
    setError(undefined);
    try {
      setBreach(await backendRef.current.securityBreachMonitorStatus(nextOptIn));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setLoading(false);
    }
  }, [setBreach, setError, setLoading, setOptIn]);

  const defenderTone = threat?.defender.status === "available" && threat.defender.realTimeEnabled
    ? "success" : "warning";
  const breachLabel = breach?.status === "requires_explicit_opt_in"
    ? "Opt-in required" : breach?.status === "ready" ? "Ready" : "Provider required";

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader>
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div><CardTitle>Security data</CardTitle><CardDescription>Local posture only. No file paths, network endpoints, account addresses, or threat names are displayed.</CardDescription></div>
            <Button size="icon" variant="outline" disabled={loading} onClick={() => void refresh()} title={hasLoaded ? "Refresh security data" : "Scan security data"} aria-label={hasLoaded ? "Refresh security data" : "Scan security data"}><Icon icon={hasLoaded ? "refresh" : "search"} className={loading ? "animate-spin" : undefined} /></Button>
          </div>
        </CardHeader>
        {error && <CardContent><p className="text-sm text-[var(--danger)]">{error}</p></CardContent>}
      </Card>

      <Card>
        <CardHeader>
          <div className="flex items-center justify-between gap-2"><CardTitle>Local threat posture</CardTitle><Badge tone={defenderTone}>{threat?.defender.status ?? "loading"}</Badge></div>
          <CardDescription>Microsoft Defender summary plus aggregate adapter activity from this device.</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-3 sm:grid-cols-3">
          <Metric label="Real-time protection" value={threat?.defender.realTimeEnabled ? "Enabled" : threat ? "Unavailable" : "Loading"} />
          <Metric label="Recent detections" value={String(threat?.defender.recentThreatCount ?? "—")} />
          <Metric label="Active adapters" value={threat ? `${threat.network.activeInterfaceCount} / ${threat.network.interfaceCount}` : "—"} />
          {Object.entries(threat?.defender.severityCounts ?? {}).map(([severity, count]) => <Metric key={severity} label={`${severity} detections`} value={String(count)} />)}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div className="flex items-center justify-between gap-2"><CardTitle>Windows CVE coverage</CardTitle><Badge tone={cve?.status === "ok" ? "success" : "warning"}>{cve?.status === "ok" ? "Available" : "Provider required"}</Badge></div>
          <CardDescription>{cve?.status === "ok" ? `${cve.results.length} bounded result${cve.results.length === 1 ? "" : "s"} for Windows ${cve.queriedVersion}.` : "OSV is pinned for package-version data, but it does not map Windows OS versions. An approved Windows provider is required."}</CardDescription>
        </CardHeader>
        <CardContent><p className="font-mono text-xs text-[var(--text-mute)]">Source: {cve?.source ?? "osv"} · updated: {cve?.sourceTimestamp ?? "—"}</p></CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div className="flex items-center justify-between gap-2"><CardTitle>Breach monitoring</CardTitle><Badge tone={breach?.status === "ready" ? "success" : "warning"}>{breachLabel}</Badge></div>
          <CardDescription>No address is accepted or sent. Monitoring stays off until a privacy-preserving Fleet provider is approved.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap items-center gap-3">
          <label className="flex items-center gap-2 text-sm text-[var(--text)]"><input type="checkbox" checked={optIn} onChange={(event) => void updateOptIn(event.target.checked)} /> I opt in to privacy-preserving breach monitoring.</label>
          <span className="font-mono text-xs text-[var(--text-mute)]">{breach?.provider ?? "fleet provider not configured"}</span>
        </CardContent>
      </Card>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return <div className="rounded-[var(--r)] border border-[var(--border)] p-3"><p className="text-xs text-[var(--text-mute)]">{label}</p><p className="mt-1 font-mono text-sm text-[var(--text)]">{value}</p></div>;
}
