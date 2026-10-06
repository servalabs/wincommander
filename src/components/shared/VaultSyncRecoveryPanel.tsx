// SPDX-License-Identifier: AGPL-3.0-or-later
import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-shell";
import useBackend from "../../hooks/useBackend";
import { Button } from "../ui/button";
import type { VaultSyncNotice } from "../../lib/vaultSyncWarning";
import { isSyncthingSetupUrl, vaultSyncRecoveryChoice, vaultSyncRecoveryReason, type VaultSyncRecoveryChoice } from "../../lib/vaultSyncRecovery";
import { personalVaultSyncError, RECOVERED_SYNC_SHARING_GUIDANCE } from "../../lib/personalVaultSyncFeedback";
import { showSuccess } from "../../utils/toast";

interface Props { notice: VaultSyncNotice; onBusyChange: (busy: boolean) => void }

export default function VaultSyncRecoveryPanel({ notice, onBusyChange }: Props) {
  const backend = useBackend();
  const latest = useRef(backend);
  latest.current = backend;
  const active = useRef(true);
  const inFlight = useRef(false);
  const [choice, setChoice] = useState<VaultSyncRecoveryChoice | null>(notice.recovery ?? null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [messages, setMessages] = useState<string[]>([]);
  const [setupUrl, setSetupUrl] = useState("");
  const [checked, setChecked] = useState(Boolean(notice.recovery));

  const inspect = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true); onBusyChange(true); setError("");
    try {
      const status = await latest.current.getEncryptedVolumeStatus();
      if (!active.current) return;
      const volumes = status.success && status.data?.volumes;
      const candidates = Array.isArray(volumes) ? volumes.filter(volume => volume.letter.toUpperCase().replace(/:$/, "") === notice.drive[0]) : [];
      const volume = candidates.length === 1 ? candidates[0] : undefined;
      if (!volume || volume.accessible !== true || volume.cleanupRequired || volume.internalDrive === undefined) throw new Error("vault_mount_state_unknown");
      const result = await latest.current.enablePersonalVaultSync(volume.internalDrive, "Sync", "inspect");
      if (!active.current) return;
      const observed = vaultSyncRecoveryChoice(volume.internalDrive, result.recovery_roots);
      if (result.recovery_required && !observed) throw new Error("vault_broker_rejected");
      setChoice(observed); setChecked(true);
      if (result.pairing_required) setMessages(current => [...current,
        `A recreated sync folder needs to be shared with your phone again. ${RECOVERED_SYNC_SHARING_GUIDANCE}`]);
      if (isSyncthingSetupUrl(result.gui_url)) setSetupUrl(result.gui_url);
    } catch (failure) {
      if (active.current) {
        setChoice(null); setChecked(false);
        setError(`${personalVaultSyncError(failure)} Check again to refresh the recovery choices before retrying.`);
      }
    } finally {
      inFlight.current = false;
      if (active.current) { setBusy(false); onBusyChange(false); }
    }
  }, [notice.drive, onBusyChange]);

  useEffect(() => {
    active.current = true;
    if (!notice.recovery) void inspect();
    return () => { active.current = false; };
  }, [inspect, notice.recovery]);

  const recreate = async (token: string) => {
    if (!choice || inFlight.current) return;
    const root = choice.roots.find(item => item.token === token);
    if (!root) return;
    inFlight.current = true; setBusy(true); onBusyChange(true); setError("");
    try {
      const result = await latest.current.enablePersonalVaultSync(choice.internalDrive, root.relative_path, "recreate", root.token);
      if (!active.current) return;
      if (!result.enabled || result.recovery_required || result.pairing_required !== true) throw new Error("vault_broker_rejected");
      const message = `${root.relative_path}: sync setup recreated. ${RECOVERED_SYNC_SHARING_GUIDANCE} Existing files were kept; deleted files were not restored.`;
      setMessages(current => [...current, message]);
      setChoice(current => current ? { ...current, roots: current.roots.filter(item => item.token !== token) } : null);
      if (isSyncthingSetupUrl(result.gui_url)) setSetupUrl(result.gui_url);
      showSuccess(message, undefined, { kind: "notification" });
    } catch (failure) {
      if (active.current) {
        setChoice(null); setChecked(false);
        setError(`${personalVaultSyncError(failure)} Check again to refresh the recovery choices before retrying.`);
      }
    } finally {
      inFlight.current = false;
      if (active.current) { setBusy(false); onBusyChange(false); }
    }
  };

  const keepPaused = async (token: string) => {
    if (!choice || inFlight.current) return;
    const root = choice.roots.find(item => item.token === token);
    if (!root) return;
    inFlight.current = true; setBusy(true); onBusyChange(true); setError("");
    try {
      const result = await latest.current.enablePersonalVaultSync(choice.internalDrive, root.relative_path, "keep_paused", root.token);
      if (!active.current) return;
      const message = `${root.relative_path}: kept paused. This does not stop Syncthing or pause this Vault’s other sync folders.`;
      setMessages(current => [...current, message]);
      setChoice(current => current ? { ...current, roots: current.roots.filter(item => item.token !== token) } : null);
      if (isSyncthingSetupUrl(result.gui_url)) setSetupUrl(result.gui_url);
      showSuccess(message, undefined, { kind: "notification" });
    } catch (failure) {
      if (active.current) {
        setChoice(null); setChecked(false);
        setError(`${personalVaultSyncError(failure)} Check again to refresh the recovery choices before retrying.`);
      }
    } finally {
      inFlight.current = false;
      if (active.current) { setBusy(false); onBusyChange(false); }
    }
  };

  return <div className="space-y-3 text-sm">
    <p>Recreating makes a new sync setup for the selected folder. Existing files stay in place; deleted files are not restored. You must share the new folder with your phone and accept it there again. Your paired devices stay saved. Keeping it paused leaves only this folder paused; it does not stop Syncthing, block the mounted Vault, or pause other folders.</p>
    {busy && <p role="status">Checking or updating this Vault’s sync setup…</p>}
    {choice?.roots.map(root => <div key={root.token} className="rounded-md border p-3 space-y-2">
      <p className="font-medium break-words">{notice.drive}\{root.relative_path}</p>
      <p>{vaultSyncRecoveryReason(root.reason)}</p>
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" disabled={busy} onClick={() => void keepPaused(root.token)}>Keep paused</Button>
        <Button disabled={busy} onClick={() => void recreate(root.token)}>Recreate sync setup</Button>
      </div>
    </div>)}
    {checked && !choice?.roots.length && !messages.length && <p>No sync folders currently need recreation.</p>}
    {messages.map(message => <p role="status" key={message}>{message}</p>)}
    {error && <div role="alert"><p>{error}</p><Button variant="outline" disabled={busy} onClick={() => void inspect()}>Check again</Button></div>}
    {setupUrl && <Button variant="outline" disabled={busy} onClick={() => void open(setupUrl).catch(() => setError("Syncthing’s setup page could not open. Try again from the Vault’s sync button."))}>Open Syncthing</Button>}
  </div>;
}
