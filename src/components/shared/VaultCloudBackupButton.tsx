// SPDX-License-Identifier: AGPL-3.0-or-later
import { useState } from "react";
import { Button } from "@/components/ui/bp";
import useBackend from "@/hooks/useBackend";
import TierGate from "./TierGate";
import VaultCloudBackupDialog from "./VaultCloudBackupDialog";
import VaultOperationNotice from "./VaultOperationNotice";

interface Props { driveLabel: string; internalDrive?: number; personal?: boolean; disabled?: boolean; disabledReason?: string; compact?: boolean }
export default function VaultCloudBackupButton({ driveLabel, internalDrive, personal = true, disabled, disabledReason, compact }: Props) {
  const { getEncryptedVolumeStatus } = useBackend();
  const [slot, setSlot] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const open = async () => {
    if (busy || disabled) return;
    setError(""); setBusy(true);
    try {
      if (internalDrive !== undefined) { setSlot(internalDrive); return; }
      const status = await getEncryptedVolumeStatus();
      const letter = driveLabel.replace(/:$/, "").toUpperCase();
      const matches = status.success ? status.data?.volumes?.filter(volume => volume.letter.replace(/:$/, "").toUpperCase() === letter
        && volume.accessible !== false && !volume.cleanupRequired && volume.internalDrive !== undefined) : [];
      if (matches?.length !== 1) throw new Error("unavailable");
      setSlot(matches[0]!.internalDrive!);
    } catch { setError("This Vault’s mounted identity could not be confirmed. Refresh Secure Storage and try again."); }
    finally { setBusy(false); }
  };
  return <>
    <TierGate tier="paid" featureLabel="Cloud backup">
      <Button icon="cloud" small minimal disabled={disabled || busy} title={disabledReason ?? "Cloud backup to Proton Drive"}
        aria-label={`Cloud backup for ${driveLabel}`} onClick={() => void open()}>{compact ? undefined : "Cloud backup"}</Button>
    </TierGate>
    <VaultOperationNotice message={error} />
    {slot !== null && <VaultCloudBackupDialog isOpen onClose={() => setSlot(null)} internalDrive={slot} driveLabel={driveLabel} personal={personal} />}
  </>;
}
