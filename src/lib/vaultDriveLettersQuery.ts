import type { AuthMode } from "../context/AuthModeContext";
import type { QueryClient } from "@tanstack/react-query";

const queryKey = (mode: AuthMode) => ["vault", "available-drive-letters", mode] as const;

export function vaultDriveLettersQueryOptions(mode: AuthMode, preload: boolean, read: () => Promise<string[]>) {
  return {
    queryKey: queryKey(mode),
    queryFn: read,
    enabled: preload && mode !== "decoy",
    staleTime: 30_000,
    refetchInterval: preload && mode !== "decoy" ? 30_000 : false as const,
    refetchIntervalInBackground: false,
    refetchOnMount: preload ? "always" as const : false as const,
    refetchOnWindowFocus: preload ? "always" as const : false as const,
    retry: false,
  };
}

export async function refreshVaultDriveLetters<T>(client: QueryClient, mode: AuthMode, read: () => Promise<T>): Promise<T> {
  // An older in-flight read must not supply the post-cleanup availability.
  await client.cancelQueries({ queryKey: queryKey(mode), exact: true });
  return read();
}
