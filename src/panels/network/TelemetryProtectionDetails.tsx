import type { BlocklistStatus } from "@/hooks/useBackend";
import { hostsVerificationLabel } from "@/lib/hostsProtection";
import { Button } from "@/components/ui/button";
import { InfoPopover } from "./InfoTip";

export function TelemetryProtectionDetails({ status, disabled, onRepair }: {
  status: BlocklistStatus | null;
  disabled: boolean;
  onRepair: () => void;
}) {
  const verification = status?.available.find(item => item.name === "telemetry-blocklist")?.verification;
  const policy = status?.telemetryPolicy;
  const needsRepair = verification?.state === "attention" || verification?.state === "partial";
  return (
    <div className="flex flex-col gap-1 text-xs text-[var(--text-dim)]">
      <div className="flex flex-wrap items-center gap-2">
        <span role="status">{hostsVerificationLabel(verification)}</span>
        <InfoPopover title="Telemetry protection" iconOnly>
          <p>{policy?.summary || "Blocks selected analytics and crash-reporting destinations."}</p>
          <p>Some apps may lose analytics or automatic error reporting. This check verifies hosts entries; it cannot prove that all tracking is blocked or that every app works.</p>
          <p>Reviewed lists arrive with WinCom updates. Windows Update, Defender communication and internet-detection destinations are excluded from the current telemetry list.</p>
          {policy && <p>Last scope review: {policy.reviewedAt}. List revision: {policy.revision}.</p>}
          <details>
            <summary>Review references</summary>
            <ul className="mt-1 list-disc pl-4">
              <li>Microsoft Windows 11 connection endpoints</li>
              <li>Microsoft Defender for Endpoint connectivity requirements</li>
            </ul>
          </details>
        </InfoPopover>
      </div>
      {needsRepair && <>
        <span>{verification.message}</span>
        <Button size="sm" variant="ghost" className="self-start" disabled={disabled} onClick={onRepair}>Update protection</Button>
      </>}
    </div>
  );
}
