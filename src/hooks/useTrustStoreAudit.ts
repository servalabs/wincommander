import { invoke } from "@tauri-apps/api/core";
import { useCallback, useState } from "react";

export type TrustCertificate = {
  scope: string;
  store: string;
  thumbprint: string;
  subject: string;
  issuer: string;
  serialNumber: string;
  notBefore: string;
  notAfter: string;
  signatureAlgorithm: string;
  publicKeyAlgorithm: string;
  hasPrivateKey: boolean;
  inWindowsAuthRoot: boolean;
};

type TrustStoreAudit = {
  certificates: TrustCertificate[];
  referenceAvailable: boolean;
  windowsAuthRootCount: number;
};

export function useTrustStoreAudit() {
  const [audit, setAudit] = useState<TrustStoreAudit | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const inspect = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      setAudit(await invoke<TrustStoreAudit>("trust_store_audit"));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }, []);

  return { audit, busy, error, inspect };
}
