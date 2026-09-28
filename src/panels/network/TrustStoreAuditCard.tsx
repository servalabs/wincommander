import { invoke } from "@tauri-apps/api/core";
import { useCallback, useMemo, useState } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import EmptyState from "../../components/shared/EmptyState";
import { InfoPopover, InfoTip } from "./InfoTip";
import { MaintenanceNotice, TableSkeleton } from "./MaintenanceNotice";

type TrustCertificate = {
  scope: string;
  store: string;
  thumbprint: string;
  subject: string;
  issuer: string;
  serialNumber: string;
  notBefore: string;
  notAfter: string;
  signatureAlgorithm: string;
  publicKeyAlgorithm: string;
  hasPrivateKey: boolean;
  inWindowsAuthRoot: boolean;
};

type TrustStoreAudit = {
  certificates: TrustCertificate[];
  referenceAvailable: boolean;
  windowsAuthRootCount: number;
};

type BaselineEntry = Pick<TrustCertificate, "scope" | "store" | "thumbprint" | "subject">;
const BASELINE_KEY = "wincommander.trust-store-baseline.v1";

function identity(cert: Pick<TrustCertificate, "scope" | "store" | "thumbprint">): string {
  return `${cert.scope}|${cert.store}|${cert.thumbprint.toUpperCase()}`;
}

function loadBaseline(): BaselineEntry[] | null {
  try {
    const raw = localStorage.getItem(BASELINE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

function likelyOwner(subject: string): string | null {
  if (/kaspersky/i.test(subject)) return "Kaspersky";
  if (/adguard/i.test(subject)) return "AdGuard";
  if (/zscaler/i.test(subject)) return "Zscaler";
  if (/bitdefender/i.test(subject)) return "Bitdefender";
  if (/eset/i.test(subject)) return "ESET";
  return null;
}

function simpleName(subject: string): string {
  const cn = subject.match(/(?:^|,\s*)CN=([^,]+)/i)?.[1]?.trim();
  return cn || subject || "Unnamed certificate";
}

export function TrustStoreAuditCard() {
  const [audit, setAudit] = useState<TrustStoreAudit | null>(null);
  const [baseline, setBaseline] = useState<BaselineEntry[] | null>(() => loadBaseline());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showAll, setShowAll] = useState(false);

  const inspect = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      setAudit(await invoke<TrustStoreAudit>("trust_store_audit"));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }, []);

  const roots = useMemo(
    () => (audit?.certificates ?? []).filter((cert) => cert.store === "Root"),
    [audit],
  );
  const baselineIds = useMemo(
    () => new Set((baseline ?? []).map(identity)),
    [baseline],
  );
  const currentIds = useMemo(() => new Set(roots.map(identity)), [roots]);
  const added = useMemo(
    () => baseline ? roots.filter((cert) => !baselineIds.has(identity(cert))) : [],
    [baseline, baselineIds, roots],
  );
  const removed = useMemo(
    () => baseline ? baseline.filter((cert) => !currentIds.has(identity(cert))) : [],
    [baseline, currentIds],
  );
  const outsideWindowsReference = useMemo(
    () => roots.filter((cert) => !cert.inWindowsAuthRoot),
    [roots],
  );
  const notable = useMemo(() => {
    const map = new Map<string, TrustCertificate>();
    for (const cert of [...added, ...outsideWindowsReference, ...roots.filter((item) => likelyOwner(item.subject))]) {
      map.set(identity(cert), cert);
    }
    return [...map.values()];
  }, [added, outsideWindowsReference, roots]);
  const visible = showAll ? roots : notable;

  const saveBaseline = useCallback(() => {
    const next = roots.map(({ scope, store, thumbprint, subject }) => ({ scope, store, thumbprint, subject }));
    localStorage.setItem(BASELINE_KEY, JSON.stringify(next));
    setBaseline(next);
  }, [roots]);

  return (
    <Card className="min-w-0">
      <CardHeader>
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div className="min-w-0">
            <CardTitle className="flex items-center gap-1.5">
              Trust store audit
              <InfoTip
                label="What this audit means"
                content={
                  <span>
                    Shows trusted CA certificates and changes to them. A certificate outside the
                    Windows AuthRoot reference is <strong>not automatically malicious</strong>:
                    security software, enterprise policy, and local tools can add legitimate roots.
                  </span>
                }
              />
            </CardTitle>
            <CardDescription className="mt-1">
              See who can add TLS trust to this PC, and what changed since your saved baseline.
            </CardDescription>
          </div>
          {audit ? (
            <Button variant="ghost" size="icon" onClick={() => void inspect()} title="Rescan trust stores">
              <Icon icon="refresh" />
            </Button>
          ) : null}
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {error ? (
          <MaintenanceNotice tone="danger" headline="Trust-store scan failed">
            {error}
          </MaintenanceNotice>
        ) : null}

        {busy ? <TableSkeleton label="Reading Windows certificate stores…" /> : null}

        {!audit && !busy && !error ? (
          <EmptyState
            icon="shield"
            title="Trust stores not audited yet"
            hint="Reads machine and current-user Root, Intermediate CA, and Trusted Publisher stores. Nothing is changed."
            action={
              <Button variant="primary" onClick={() => void inspect()}>
                <Icon icon="search" />
                Audit trust stores
              </Button>
            }
          />
        ) : null}

        {audit && !busy ? (
          <>
            <div className="flex flex-wrap gap-2">
              <Badge tone="neutral">{roots.length} trusted roots</Badge>
              {audit.referenceAvailable ? (
                <Badge tone={outsideWindowsReference.length ? "warning" : "success"}>
                  {outsideWindowsReference.length} outside Windows AuthRoot
                </Badge>
              ) : (
                <Badge tone="warning">Windows AuthRoot reference unavailable</Badge>
              )}
              {baseline ? (
                <>
                  <Badge tone={added.length ? "warning" : "success"}>{added.length} added since baseline</Badge>
                  <Badge tone={removed.length ? "warning" : "neutral"}>{removed.length} removed</Badge>
                </>
              ) : (
                <Badge tone="neutral">baseline not saved</Badge>
              )}
            </div>

            {!baseline ? (
              <MaintenanceNotice tone="primary" headline="Save a known-good baseline">
                Windows roots change over time. Save the current state only after you recognise the
                certificates you expect; later scans will show additions and removals.
              </MaintenanceNotice>
            ) : null}

            {visible.length === 0 ? (
              <MaintenanceNotice tone="success" headline="No notable trust changes">
                No new roots were found since the saved baseline, and no machine roots fall outside
                the current Windows AuthRoot reference.
              </MaintenanceNotice>
            ) : (
              <div className="overflow-x-auto rounded-md border border-[var(--border)]">
                <table className="w-full min-w-[760px] border-collapse text-left text-[11px]">
                  <thead className="bg-[var(--surface-2)] text-[var(--text-mute)]">
                    <tr>
                      <th className="px-3 py-2 font-semibold">Certificate</th>
                      <th className="px-3 py-2 font-semibold">Scope</th>
                      <th className="px-3 py-2 font-semibold">Why shown</th>
                      <th className="px-3 py-2 font-semibold">Expires</th>
                      <th className="px-3 py-2 font-semibold">SHA-1 thumbprint</th>
                    </tr>
                  </thead>
                  <tbody>
                    {visible.map((cert) => {
                      const owner = likelyOwner(cert.subject);
                      const isAdded = baselineIds.size > 0 && !baselineIds.has(identity(cert));
                      const outsideRef = !cert.inWindowsAuthRoot;
                      const why = [
                        isAdded ? "added since baseline" : null,
                        outsideRef ? "outside Windows AuthRoot" : null,
                        owner ? `${owner} certificate` : null,
                      ].filter(Boolean).join(" · ") || "trusted root";
                      return (
                        <tr key={identity(cert)} className="border-t border-[var(--border)] align-top">
                          <td className="px-3 py-2">
                            <div className="font-semibold text-[var(--text)]">{simpleName(cert.subject)}</div>
                            <div className="mt-0.5 max-w-[340px] break-words text-[10px] text-[var(--text-mute)]">{cert.issuer}</div>
                          </td>
                          <td className="px-3 py-2 font-mono text-[10px]">{cert.scope}</td>
                          <td className="px-3 py-2">{why}</td>
                          <td className="px-3 py-2">{new Date(cert.notAfter).toLocaleDateString()}</td>
                          <td className="max-w-[220px] break-all px-3 py-2 font-mono text-[9px] text-[var(--text-mute)]">
                            {cert.thumbprint}
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )}

            {removed.length > 0 ? (
              <MaintenanceNotice tone="warning" headline={`${removed.length} baseline root${removed.length === 1 ? "" : "s"} no longer present`}>
                {removed.slice(0, 4).map((cert) => simpleName(cert.subject)).join(", ")}
                {removed.length > 4 ? ` and ${removed.length - 4} more` : ""}
              </MaintenanceNotice>
            ) : null}

            <div className="flex flex-wrap items-center justify-between gap-2 border-t border-[var(--border)] pt-3">
              <p className="max-w-[620px] text-[10px] leading-relaxed text-[var(--text-mute)]">
                The Windows AuthRoot comparison is a transparency hint, not a verdict. Enterprise
                roots, HTTPS inspection tools, VPN/security software, and private PKI can be legitimate.
              </p>
              <div className="flex flex-wrap gap-2">
                <Button variant="outline" size="sm" onClick={() => setShowAll((value) => !value)}>
                  {showAll ? "Show notable only" : "Show all trusted roots"}
                </Button>
                <Button variant="outline" size="sm" onClick={saveBaseline}>
                  <Icon icon="floppy-disk" />
                  {baseline ? "Replace baseline" : "Save baseline"}
                </Button>
              </div>
            </div>
          </>
        ) : null}

        <InfoPopover title="Certificate trust transparency">
          <p>
            <strong className="text-[var(--text)]">Root:</strong> certificates that can anchor TLS
            trust for this user or machine.
          </p>
          <p>
            <strong className="text-[var(--text)]">Windows AuthRoot:</strong> the Windows-maintained
            third-party root reference. A mismatch deserves review but is not proof of compromise.
          </p>
          <p>
            <strong className="text-[var(--text)]">Baseline:</strong> your locally saved snapshot.
            WinCommander never deletes certificates from this audit screen.
          </p>
        </InfoPopover>
      </CardContent>
    </Card>
  );
}
