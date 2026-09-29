import { useCallback } from "react";
import { useQuery } from "@tanstack/react-query";
import { useAuthMode } from "../context/AuthModeContext";
import useBackend from "./useBackend";
import { selectableDriveLetters } from "@/lib/vaultOperationFeedback";
import { waitForMountOptions } from "../panels/vault/mountOperationProgress";

const EMPTY_LETTERS: string[] = [];

// AppContext owns background reads; opening a picker only subscribes to this cache.
export default function useVaultDriveLetters(preload = false) {
  const { mode } = useAuthMode();
  const { getAvailableDriveLetters } = useBackend();
  const query = useQuery({
    queryKey: ["vault", "available-drive-letters", mode],
    queryFn: async () => {
      const result = await waitForMountOptions(getAvailableDriveLetters());
      if (!result.success || !result.data) throw new Error("vault_drive_letters_unavailable");
      return selectableDriveLetters(result.data.letters);
    },
    enabled: preload && mode !== "decoy",
    staleTime: 30_000,
    refetchInterval: preload && mode !== "decoy" ? 30_000 : false,
    refetchIntervalInBackground: false,
    refetchOnMount: preload ? "always" : false,
    refetchOnWindowFocus: false,
    retry: false,
  });
  const { refetch } = query;
  const refresh = useCallback(async () => {
    if (mode === "decoy") return null;
    const result = await refetch({ cancelRefetch: false });
    return result.isError ? null : result.data ?? null;
  }, [mode, refetch]);
  return {
    letters: mode === "decoy" || query.isError ? EMPTY_LETTERS : query.data ?? EMPTY_LETTERS,
    loading: mode !== "decoy" && (query.isPending || query.isFetching),
    unavailable: mode === "decoy" || query.isError,
    refresh,
  };
}
