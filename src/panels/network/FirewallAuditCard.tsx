// Firewall audit — one card: header, reading of the result, candidate rules,
// footer actions, and the post-change report (per-rule failures + the .wfw
// backup path, both of which the previous version computed and discarded).
import { useMemo, useState } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import EmptyState from "../../components/shared/EmptyState";
import { firewallConfirmCopy } from "../../lib/firewallAuditCopy";
import { summarizeFirewallAudit, type FirewallAction } from "../../lib/firewallAuditView";
import { DangerConfirmDialog } from "./DangerConfirmDialog";
import { InfoPopover, InfoTip } from "./InfoTip";
import { FirewallRemediationReport } from "./FirewallRemediationReport";
import { FirewallRulesTable } from "./FirewallRulesTable";
import { MaintenanceNotice, TableSkeleton } from "./MaintenanceNotice";
import type { useFirewallAudit } from "./useFirewallAudit";

export function FirewallAuditCard({ firewall }: { firewall: ReturnType<typeof useFirewallAudit> }) {
  const [pending, setPending] = useState<{ action: FirewallAction; names: string[] } | null>(null);

  const { audit, rules, report } = firewall;
  const summary = useMemo(
    () => (audit ? summarizeFirewallAudit(rules, audit.cancelled) : null),
    [audit, rules],
  );
  const selectedCount = firewall.selected.size;
  const confirm = pending ? firewallConfirmCopy(pending.action, pending.names) : null;

  const request = (action: FirewallAction) =>
    setPending({ action, names: firewall.selectedRules.map((rule) => rule.name) });

  return (
    <Card className="min-w-0">
      <CardHeader>
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div className="min-w-0">
            <CardTitle className="flex items-center gap-1.5">
              Firewall audit
              <InfoTip
                label="What this audit looks for"
                content={
                  <span>
                    <strong>What it flags:</strong> third-party rules that allow all traffic, or
                    are switched off when they should be blocking something.{" "}
                    <strong>Never touched:</strong> Windows, Microsoft, Defender, and WinCommander's
                    own rules.
                  </span>
                }
              />
            </CardTitle>
            <CardDescription className="mt-1">
              Reviews the firewall rules other software has added, and tells you which ones are worth
              a decision.
            </CardDescription>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            {audit && !firewall.busy ? (
              <Button
                variant="ghost"
                size="icon"
                onClick={() => void firewall.inspect()}
                title="Re-audit firewall rules"
                aria-label="Re-audit firewall rules"
              >
                <Icon icon="refresh" />
              </Button>
            ) : null}
            <InfoPopover title="Firewall audit">
              <p>
                <strong className="text-[var(--text)]">What it flags:</strong> rules that allow
                traffic for every program, and rules meant to block something but switched off — a
                disabled block rule is protection that silently stopped working.
              </p>
              <p>
                <strong className="text-[var(--text)]">Never touched:</strong> rules belonging to
                Windows, Microsoft, Defender, or WinCommander itself are excluded and can never be
                changed from here.
              </p>
              <p>
                <strong className="text-[var(--text)]">Disable vs. Remove:</strong> disabling is
                reversible with one click; removing is not — prefer Disable.
              </p>
              <p>
                <strong className="text-[var(--text)]">Backup first:</strong> WinCommander exports a
                complete firewall backup (
                <span className="font-[family-name:var(--font-mono)]">.wfw</span>) before it changes
                anything, so the whole rule set can be restored.
              </p>
            </InfoPopover>
          </div>
        </div>
      </CardHeader>

      <CardContent className="flex flex-col gap-3">
        {firewall.error ? (
          <MaintenanceNotice
            tone="danger"
            headline={firewall.error.title}
            action={
              <Button variant="outline" size="sm" disabled={firewall.busy} onClick={() => void firewall.inspect()}>
                <Icon icon="refresh" />
                Try again
              </Button>
            }
          >
            {firewall.error.hint}
          </MaintenanceNotice>
        ) : null}

        {firewall.auditing ? (
          <div className="flex flex-col gap-2">
            <TableSkeleton
              label="Asking Windows for every firewall rule — this can take a moment on a busy machine…"
            />
            <div>
              <Button variant="outline" size="sm" onClick={() => void firewall.cancel()}>
                <Icon icon="stop" />
                Cancel audit
              </Button>
            </div>
          </div>
        ) : null}

        {!audit && !firewall.auditing && !firewall.error ? (
          <EmptyState
            icon="shield"
            title="Firewall rules not audited yet"
            hint="Lists the third-party rules that let traffic through or that have been switched off, with a recommendation for each."
            action={
              <Button variant="primary" onClick={() => void firewall.inspect()}>
                <Icon icon="shield" />
                Audit firewall rules
              </Button>
            }
          />
        ) : null}

        {audit && !firewall.auditing && summary ? (
          <>
            <MaintenanceNotice tone={summary.intent} headline={summary.headline}>
              {summary.detail}
            </MaintenanceNotice>

            {rules.length > 0 ? (
              <>
                <div className="flex flex-wrap items-center gap-2">
                  <Badge tone="neutral">{rules.length} to review</Badge>
                  {selectedCount > 0 ? <Badge tone="accent">{selectedCount} selected</Badge> : null}
                  {audit.cancelled ? <Badge tone="warning">partial</Badge> : null}
                </div>

                <FirewallRulesTable
                  rules={rules}
                  selected={firewall.selected}
                  disabled={firewall.busy}
                  onToggle={firewall.toggle}
                  onSetSelection={firewall.setSelection}
                />

                <div className="flex flex-wrap items-center justify-between gap-2 border-t border-[var(--border)] pt-3">
                  <p className="text-[11px] leading-relaxed text-[var(--text-mute)]">
                    {selectedCount === 0
                      ? "Select the rules you don't recognise. Disabling is reversible; removing is not."
                      : "A full firewall backup is exported before any change."}
                  </p>
                  <div className="flex flex-wrap gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={firewall.busy || selectedCount === 0}
                      onClick={() => request("enable")}
                    >
                      <Icon
                        icon={firewall.applying === "enable" ? "refresh" : "tick"}
                        className={firewall.applying === "enable" ? "animate-spin" : undefined}
                      />
                      Enable ({selectedCount})
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={firewall.busy || selectedCount === 0}
                      onClick={() => request("disable")}
                    >
                      <Icon
                        icon={firewall.applying === "disable" ? "refresh" : "disable"}
                        className={firewall.applying === "disable" ? "animate-spin" : undefined}
                      />
                      Disable ({selectedCount})
                    </Button>
                    <Button
                      variant="danger"
                      size="sm"
                      disabled={firewall.busy || selectedCount === 0}
                      onClick={() => request("remove")}
                    >
                      <Icon
                        icon={firewall.applying === "remove" ? "refresh" : "trash"}
                        className={firewall.applying === "remove" ? "animate-spin" : undefined}
                      />
                      Remove ({selectedCount})
                    </Button>
                  </div>
                </div>
              </>
            ) : null}
          </>
        ) : null}

        {report ? <FirewallRemediationReport report={report} /> : null}
      </CardContent>

      <DangerConfirmDialog
        open={pending !== null}
        onOpenChange={(next) => {
          if (!next) setPending(null);
        }}
        onConfirm={() => {
          const action = pending?.action;
          setPending(null);
          if (action) void firewall.remediate(action);
        }}
        title={confirm?.title ?? ""}
        intro={confirm?.intro ?? ""}
        consequences={confirm?.consequences ?? []}
        actionLabel={confirm?.actionLabel ?? ""}
        countdownSeconds={confirm?.countdownSeconds ?? 0}
      />
    </Card>
  );
}
