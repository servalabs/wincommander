import { Switch, Tag } from "@/components/ui/bp";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { UsbHidApprovalGateStatus } from "../../lib/usbHidApproval";

const TTL_OPTIONS = [30, 60, 300] as const;

interface UsbHidApprovalGateSettingsProps {
  enabled: boolean;
  ttlSecs: number;
  status: UsbHidApprovalGateStatus | null;
  busy: boolean;
  onEnabledChange: (enabled: boolean) => void;
  onTtlChange: (ttlSecs: number) => void;
}

export default function UsbHidApprovalGateSettings({
  enabled,
  ttlSecs,
  status,
  busy,
  onEnabledChange,
  onTtlChange,
}: UsbHidApprovalGateSettingsProps) {
  const active = enabled && status?.running === true;
  return (
    <div className="border-t border-white/10 pt-3 flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-3">
        <div className="text-sm font-semibold">New keyboard approval</div>
        <Switch
          checked={enabled}
          disabled={busy}
          onChange={(event) => onEnabledChange((event.target as HTMLInputElement).checked)}
          label="Require a human-presence click challenge for new keyboards"
        />
        <Tag minimal intent={active ? "success" : "none"} className="font-mono">
          {active ? "ACTIVE" : "OFF"}
        </Tag>
      </div>
      <p className="text-xs text-[var(--text-dim)]">
        A newly detected keyboard stays blocked until an operator completes a visual click challenge for Allow once or Always trust. No response defaults to blocked.
      </p>
      <p className="text-xs text-[var(--text-mute)]">
        This is defense in depth, not a claim that WinCommander can identify a separate known mouse. It is reactive after Windows detects the device and does not claim to prevent a first keystroke or pre-boot input.
      </p>
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <span className="text-[var(--text-dim)]">Approval window:</span>
        <Select
          value={String(ttlSecs)}
          disabled={busy || !enabled}
          onValueChange={(value) => onTtlChange(Number(value))}
        >
          <SelectTrigger aria-label="Keyboard approval window" className="w-32">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {TTL_OPTIONS.map((option) => (
              <SelectItem key={option} value={String(option)}>{option} seconds</SelectItem>
            ))}
          </SelectContent>
        </Select>
        {status && <span className="text-[var(--text-mute)]">{status.pendingCount} awaiting decision</span>}
      </div>
    </div>
  );
}
