// State + IPC for the selective crypto-erase section. Fetches BitLocker
// volumes, merges them with the VeraCrypt volumes the vault panel already
// holds (pure buildTargets), and runs a single-target erase, keeping the
// latest receipt per target for inline display. No cascade — one target,
// one Pro call, via useBackend.eraseEncryptedContainer.
import { useCallback, useEffect, useMemo, useState } from "react";
import useBackend, { type EraseReceipt, type BitLockerVolume } from "./useBackend";
import { buildTargets, type EncryptedTarget } from "../lib/cryptoEraseTargets";

type VeraVolume = { letter: string; path: string | null; type: string };

export function useCryptoErase(veracryptVolumes: VeraVolume[]) {
  const { getBitLockerVolumes, eraseEncryptedContainer } = useBackend();
  const [bitlocker, setBitlocker] = useState<BitLockerVolume[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [receipts, setReceipts] = useState<Record<string, EraseReceipt>>({});

  const systemDrive = "C:"; // display hint; the server re-derives from %SystemDrive%

  const refresh = useCallback(async () => {
    setRefreshing(true);
    const res = await getBitLockerVolumes();
    setBitlocker(res.success && Array.isArray(res.data) ? res.data : []);
    setRefreshing(false);
    setLoading(false);
  }, [getBitLockerVolumes]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const targets = useMemo(
    () => buildTargets(veracryptVolumes, bitlocker, systemDrive),
    [veracryptVolumes, bitlocker],
  );

  const erase = useCallback(
    async (target: EncryptedTarget, osAck?: string): Promise<EraseReceipt | null> => {
      const res = await eraseEncryptedContainer({
        kind: target.kind,
        path: target.path,
        mountLetter: target.mountLetter,
        mountPoint: target.mountPoint,
        confirmed: true,
        osVolumeAck: osAck,
      });
      if (res.success && res.data) {
        setReceipts((prev) => ({ ...prev, [target.id]: res.data as EraseReceipt }));
        await refresh();
        return res.data;
      }
      throw new Error(res.error || "Crypto-erase failed");
    },
    [eraseEncryptedContainer, refresh],
  );

  return { targets, loading, refreshing, receipts, refresh, erase };
}

export default useCryptoErase;
