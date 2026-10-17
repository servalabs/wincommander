import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { useConnectionCheck } from "./useConnectionCheck";
import { MaintenanceNotice } from "./MaintenanceNotice";

export function ConnectionCheckCard() {
  const check = useConnectionCheck();
  return (
    <Card className="min-w-0">
      <CardHeader>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <CardTitle>Connection check</CardTitle>
          <Button size="sm" disabled={check.busy} onClick={() => void check.inspect()}>
            {check.busy ? "Checking…" : "Check connection"}
          </Button>
        </div>
        <CardDescription>Checks DNS, a Microsoft HTTPS destination, Windows internet status and essential hosts entries. Settings stay unchanged.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3" aria-busy={check.busy}>
        {check.error && <div role="alert"><MaintenanceNotice tone="danger" headline="Could not complete the action">{check.error}</MaintenanceNotice></div>}
        {check.result && <div role="status" className="flex flex-col gap-2">
          <p className="text-xs text-[var(--text-mute)]">Checked {new Date(check.result.checkedAt).toLocaleTimeString()}</p>
          {check.result.checks.map(item => (
            <MaintenanceNotice key={item.name} tone={item.state === "ok" ? "success" : "warning"} headline={item.name}>
              {item.detail}
              {!!item.affectedDomains?.length && <details className="mt-1">
                <summary>Technical details</summary>
                <ul className="mt-1 max-h-40 overflow-y-auto break-all font-mono">
                  {item.affectedDomains.map(domain => <li key={domain}>{domain}</li>)}
                </ul>
              </details>}
            </MaintenanceNotice>
          ))}
          <p className="text-xs text-[var(--text-mute)]">These checks do not test every app, Windows Update or Defender end to end. A failed check alone does not identify WinCom as the cause.</p>
        </div>}
        {check.undo?.available && <div className="flex flex-wrap items-center gap-2 text-xs text-[var(--text-dim)]">
          <span>Undo is available for the last WinCom hosts change.</span>
          <Button size="sm" variant="ghost" disabled={check.busy || check.needsElevation} onClick={() => void check.undoLastChange()}>Undo last blocking change</Button>
          {check.needsElevation && <span>Administrator approval is required to undo.</span>}
        </div>}
      </CardContent>
    </Card>
  );
}
