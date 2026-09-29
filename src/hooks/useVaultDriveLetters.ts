import { useCallback, useEffect, useRef, useState } from "react";
import useBackend from "./useBackend";
import { selectableDriveLetters } from "@/lib/vaultOperationFeedback";

/** A UI observation only: every mount is revalidated by the service. */
export default function useVaultDriveLetters(active: boolean) {
  const { getAvailableDriveLetters } = useBackend();
  const [letters, setLetters] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [unavailable, setUnavailable] = useState(false);
  const revision = useRef(0);
  const refresh = useCallback(async () => {
    const request = ++revision.current;
    setLoading(true);
    setUnavailable(false);
    try {
      const result = await getAvailableDriveLetters();
      if (!result.success || !result.data) throw new Error("Drive list unavailable");
      const available = selectableDriveLetters(result.data.letters);
      if (request === revision.current) setLetters(available);
      return available;
    } catch {
      if (request === revision.current) { setLetters([]); setUnavailable(true); }
      return null;
    } finally { if (request === revision.current) setLoading(false); }
  }, [getAvailableDriveLetters]);
  useEffect(() => {
    if (active) void refresh();
    return () => { ++revision.current; };
  }, [active, refresh]);
  return { letters, loading, unavailable, refresh };
}
