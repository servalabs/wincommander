import { lazy, Suspense, useCallback, useMemo, useState } from "react";
import useEntitlements from "./useEntitlements";
import useVaultNames from "./useVaultNames";
import { resolveVaultFilePath, type VaultFileActions, type VaultFileRequest, type VaultFileTarget } from "../lib/vaultFileActions";

const VaultFileDialogs = lazy(() => import("../panels/vault/VaultFileDialogs"));

/** Change Password and Rename for any vault row; render `dialogs` once next to the rows. */
export default function useVaultFileActions() {
  const { names } = useVaultNames();
  const { canUse } = useEntitlements();
  const canChangePassword = canUse("paid");
  const [request, setRequest] = useState<VaultFileRequest | null>(null);

  const show = useCallback((dialog: VaultFileRequest["dialog"], target: VaultFileTarget) => {
    const path = resolveVaultFilePath(target, names);
    setRequest(current => ({ id: (current?.id ?? 0) + 1, dialog, path, kind: target.kind, open: true }));
  }, [names]);

  const actions = useMemo<VaultFileActions>(() => ({
    ...(canChangePassword ? { changePassword: (target: VaultFileTarget) => show("password", target) } : {}),
    rename: target => show("rename", target),
  }), [canChangePassword, show]);

  const close = useCallback(() => setRequest(current => current && { ...current, open: false }), []);

  const dialogs = request
    ? <Suspense fallback={null}><VaultFileDialogs request={request} onClose={close} /></Suspense>
    : null;
  return { actions, dialogs };
}
