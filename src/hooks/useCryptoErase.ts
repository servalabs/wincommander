// State + IPC for the selective crypto-erase section. Fetches BitLocker
// volumes, merges them with the VeraCrypt volumes the vault panel already
// holds (pure buildTargets), and runs a single-target erase.
//
// Receipts are kept in an append-only history keyed independently of `targets`.
// WHY: `erase` calls `refresh()`, and a BitLocker volume that changes state can
// drop straight out of the volume list — with receipts stored per live target
// that took the proof of an irreversible operation off screen with it.
import { useCallback, useEffect, useMemo, useState } from "react";
import useBackend, { type EraseReceipt, type BitLockerVolume } from "./useBackend";
import { buildTargets, deriveSystemDrive, type EncryptedTarget } from "../lib/cryptoEraseTargets";

type VeraVolume = { letter: string; path: string | null; type: string };

/** Client-observable phases of one erase. The Tauri command emits no progress
 *  events, so these are the only honest checkpoints available (see the backend
 *  gap noted in the section header). */
export type ErasePhase = "destroying" | "verifying" | "done";

export interface EraseRecord {
  targetId: string;
  label: string;
  /** Millisecond timestamp — kept here so formatting stays pure/testable. */
  at: number;
  receipt: EraseReceipt;
}

export function useCryptoErase(veracryptVolumes: VeraVolume[]) {
  const { getBitLockerVolumes, eraseEncryptedContainer } = useBackend();
  const [bitlocker, setBitlocker] = useState<BitLockerVolume[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [loadError, setLoadError] = useState<string>();
  const [history, setHistory] = useState<EraseRecord[]>([]);

  const refresh = useCallback(async () => {
    setRefreshing(true);
    const res = await getBitLockerVolumes();
    if (res.success && Array.isArray(res.data)) {
      setBitlocker(res.data);
      setLoadError(undefined);
    } else {
      setBitlocker([]);
      setLoadError(res.error || "Windows did not return a BitLocker volume list.");
    }
    setRefreshing(false);
    setLoading(false);
  }, [getBitLockerVolumes]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // The server derives the nuclear ack token from %SystemDrive%; the OS volume's
  // mount point is the same value, so use it instead of assuming C:.
  const systemDrive = useMemo(() => deriveSystemDrive(bitlocker), [bitlocker]);

  const targets = useMemo(
    () => buildTargets(veracryptVolumes, bitlocker, systemDrive),
    [veracryptVolumes, bitlocker, systemDrive],
  );

  /** Latest record per target id — history is append-only, so the last wins. */
  const receipts = useMemo(() => {
    const latest: Record<string, EraseRecord> = {};
    for (const record of history) latest[record.targetId] = record;
    return latest;
  }, [history]);

  const erase = useCallback(
    async (
      target: EncryptedTarget,
      osAck?: string,
      onPhase?: (phase: ErasePhase) => void,
    ): Promise<EraseReceipt> => {
      onPhase?.("destroying");
      const res = await eraseEncryptedContainer({
        kind: target.kind,
        path: target.path,
        mountLetter: target.mountLetter,
        mountPoint: target.mountPoint,
        confirmed: true,
        osVolumeAck: osAck,
      });
      if (!res.success || !res.data) throw new Error(res.error || "Crypto-erase failed");
      const receipt = res.data;
      setHistory((prev) => [
        ...prev,
        { targetId: target.id, label: target.label, at: Date.now(), receipt },
      ]);
      onPhase?.("verifying");
      await refresh();
      onPhase?.("done");
      return receipt;
    },
    [eraseEncryptedContainer, refresh],
  );

  return {
    targets,
    systemDrive,
    loading,
    refreshing,
    loadError,
    receipts,
    history,
    refresh,
    erase,
  };
}

export default useCryptoErase;
