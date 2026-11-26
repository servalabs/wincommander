// SPDX-License-Identifier: AGPL-3.0-or-later
import SectionCard from "@/components/shared/SectionCard";
import VaultCloudBackupButton from "@/components/shared/VaultCloudBackupButton";
import type { EncryptionStatus } from "@/hooks/useBackend";

interface Props { volumes: NonNullable<EncryptionStatus["volumes"]>; statusError?: string | null }
export default function CloudBackupSection({ volumes, statusError }: Props) {
  const available = volumes.filter(volume => volume.accessible !== false && !volume.cleanupRequired && volume.internalDrive !== undefined);
  return <SectionCard title="Cloud backup" icon="cloud">
    <p>Back up files from an open Vault to your own Proton Drive account. After setup, changes upload automatically while the Vault stays open.</p>
    <p className="text-sm text-muted-foreground">Your local files and backup settings stay inside the Vault. Closing it stops uploads. Remote backup files remain in Proton Drive.</p>
    {available.length === 0 && <p>Mount a Vault to set up cloud backup.</p>}
    {statusError && <p role="status">Vault status is unavailable. Refresh before setting up cloud backup.</p>}
    {available.map(volume => <div key={`${volume.internalDrive}-${volume.letter}`} className="flex items-center justify-between gap-2 py-2">
      <strong>{volume.letter}</strong>
      <span className="flex items-center gap-1">
        <VaultCloudBackupButton driveLabel={volume.letter} internalDrive={volume.internalDrive} disabled={Boolean(statusError)} />
      </span>
    </div>)}
  </SectionCard>;
}
