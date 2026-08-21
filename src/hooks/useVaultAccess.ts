import { invoke } from "@tauri-apps/api/core";
import { useCallback } from "react";

/** Typed renderer boundary for the service-owned Vault Access policy. */
export default function useVaultAccess<Policy, Status>() {
  const getPolicy = useCallback(
    () => invoke<Policy>("get_vault_access_policy"),
    [],
  );
  const getStatus = useCallback(
    () => invoke<Status>("get_vault_access_status"),
    [],
  );
  const applyPolicy = useCallback(
    (policy: Policy) => invoke<Status>("apply_vault_access_policy", { policy }),
    [],
  );

  return { getPolicy, getStatus, applyPolicy };
}
