import { Button, Icon } from "@/components/ui/bp";
import { useCallback, useEffect, useMemo, useState } from "react";
import useVaultAccess from "@/hooks/useVaultAccess";
import type { VaultAuthorizedEntry } from "@/panels/fleet/vaultAccessTypes";
import { vaultPresentationLabel } from "@/panels/fleet/vaultAccessTypes";
import SectionCard from "../../components/shared/SectionCard";
import { showError, showSuccess } from "../../utils/toast";

interface Props {
  onDismounted: () => Promise<unknown> | void;
}

/**
 * Secure Storage's caller-filtered view of Fleet-managed Vaults. This is a
 * convenience surface only: the system service remains the authority for
 * listing and dismounting entries, so this component cannot expose or evict
 * another Windows account's Vault.
 */
export default function FleetMountedVaultsSection({ onDismounted }: Props) {
  const { listAuthorizedEntries, unmountEntry } = useVaultAccess<never, never>();
  const [entries, setEntries] = useState<VaultAuthorizedEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [dismountingId, setDismountingId] = useState<string | null>(null);

  const mountedEntries = useMemo(
    // Fleet drives are visible through Secure Storage only to a caller who
    // may actually close them. The service derives `can_dismount` from the
    // live policy and Windows token; this client-side filter is presentation
    // only and never substitutes for that service enforcement.
    () => entries.filter((entry) => entry.mount_state === "mounted" && entry.can_dismount),
    [entries],
  );

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setEntries(await listAuthorizedEntries());
    } catch {
      // Do not reflect raw service errors into the UI; they can carry local
      // identity or transport information that is not useful to the operator.
      setError("Fleet Vault status could not be loaded. Refresh after the local security service is available.");
    } finally {
      setLoading(false);
    }
  }, [listAuthorizedEntries]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const dismount = useCallback(async (entry: VaultAuthorizedEntry) => {
    setDismountingId(entry.entry_id);
    try {
      const result = await unmountEntry(entry.entry_id);
      if (result.state !== "unmounted") {
        showError("The Vault could not be safely dismounted.");
        return;
      }
      await Promise.all([refresh(), Promise.resolve(onDismounted())]);
      showSuccess(`${entry.label} dismounted.`);
    } catch {
      showError("The Vault dismount request could not be completed.");
    } finally {
      setDismountingId(null);
    }
  }, [onDismounted, refresh, unmountEntry]);

  return (
    <SectionCard
      title="Mounted Fleet Vaults"
      icon="people"
      headerRight={
        <Button
          icon="refresh"
          minimal
          small
          disabled={loading}
          onClick={() => void refresh()}
          aria-label="Refresh mounted Fleet Vaults"
        />
      }
    >
      <p className="vault-fleet-description">
        Only mounted Fleet Vaults the local security service authorizes this Windows account to dismount are shown. Eligibility follows the saved policy and Vault ownership—not Windows administrator status alone—and another user’s private Vault is never affected.
      </p>
      {loading ? (
        <div className="vault-fleet-empty" role="status" aria-busy="true">
          <Icon icon="refresh" size={16} /> Checking authorized Fleet Vaults…
        </div>
      ) : error ? (
        <div className="vault-fleet-empty vault-fleet-error" role="alert">{error}</div>
      ) : mountedEntries.length === 0 ? (
        <div className="vault-fleet-empty">No authorized Fleet Vaults are mounted for this account.</div>
      ) : (
        <div className="vault-fleet-list">
          {mountedEntries.map((entry) => (
            <div className="vault-fleet-row" key={entry.entry_id}>
              <div className="min-w-0">
                <strong>{entry.label}</strong>
                <p>{entry.drive_letter ? `Mounted at ${entry.drive_letter}` : "Mounted for this Windows session"} · {vaultPresentationLabel(entry.presentation)}</p>
              </div>
              <Button
                small
                intent="danger"
                disabled={dismountingId !== null}
                onClick={() => void dismount(entry)}
                aria-label={`Dismount ${entry.label}`}
              >
                {dismountingId === entry.entry_id ? "Dismounting…" : "Dismount"}
              </Button>
            </div>
          ))}
        </div>
      )}
    </SectionCard>
  );
}
