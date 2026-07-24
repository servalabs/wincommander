// src/panels/privacy/CreateWipeUsbDialog.tsx
//
// F6 Create-Wipe-USB provisioning wizard.
//
// Binds a removable USB to THIS device by writing the device's Ed25519 public
// key (pubkey.bin) + device_id.txt to the USB's \wipe\ control area, so the
// USB-side verify-token.sh accepts the tokens this device issues.
//
// Uses direct invoke() (NOT routed through useBackend.ts). The USB must already
// carry the Microsoft-signed-shim wipe image (a signed Pro-provided artifact,
// not built from this repo); this wizard writes the
// device binding only — a blank USB with just these files is NOT bootable.

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Dialog, DialogBody, DialogFooter } from "@/components/ui/bp";
import { showError, showSuccess } from "../../utils/toast";

interface RemovableVolume {
  driveLetter: string;
  label: string;
}

interface ProvisionResult {
  pubkeyPath: string;
  deviceIdPath: string;
  deviceId: string;
}

interface Props {
  open: boolean;
  onClose: () => void;
}

export default function CreateWipeUsbDialog({ open, onClose }: Props) {
  const [volumes, setVolumes] = useState<RemovableVolume[]>([]);
  const [selected, setSelected] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const [provisioned, setProvisioned] = useState<ProvisionResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) {
      setVolumes([]);
      setSelected("");
      setProvisioned(null);
      setError(null);
      return;
    }
    invoke<RemovableVolume[]>("f6_list_removable_volumes")
      .then((vols) => {
        const list = Array.isArray(vols) ? vols : [];
        setVolumes(list);
        if (list.length > 0) setSelected(list[0].driveLetter);
      })
      .catch((e: unknown) => setError(String(e)));
  }, [open]);

  const handleProvision = useCallback(async () => {
    if (!selected) return;
    setLoading(true);
    setError(null);
    setProvisioned(null);
    try {
      const result = await invoke<ProvisionResult>("f6_provision_wipe_usb", {
        usbRoot: selected,
      });
      setProvisioned(result);
      showSuccess("Wipe USB provisioned — device binding written.");
    } catch (e: unknown) {
      const msg = String(e);
      setError(msg);
      showError(`Provision failed: ${msg}`);
    } finally {
      setLoading(false);
    }
  }, [selected]);

  return (
    <Dialog
      isOpen={open}
      onClose={onClose}
      title="Create Wipe USB — Device Binding"
      icon="warning-sign"
      className="mount-dialog"
      isCloseButtonShown
      canEscapeKeyClose={!loading}
      canOutsideClickClose={!loading}
    >
      <DialogBody>
        <p className="text-[var(--text-dim)] text-sm mb-3">
          Writes this device&rsquo;s public key and ID to the selected USB so it
          will only ever wipe <strong>this</strong> machine.
        </p>

        <div className="mb-3 rounded-[var(--r-lg)] border border-[var(--border)] bg-[var(--surface-2)]/40 px-3 py-2.5 text-xs text-[var(--text-dim)] leading-relaxed">
          <span className="font-semibold text-[var(--text)]">Prerequisite:</span>{" "}
          the USB must already carry the Microsoft-signed-shim wipe image, a{" "}
          signed, Pro-provided USB artifact. A blank USB with only these binding files is{" "}
          <strong>not bootable</strong> — write the image first.
        </div>

        <div className="mb-4 rounded-[var(--r-lg)] border border-[var(--warn)]/30 bg-[var(--warn)]/8 px-3 py-2.5 text-xs text-[var(--text-dim)] leading-relaxed">
          <span className="font-semibold text-[var(--warn)]">Irreversible context.</span>{" "}
          Once provisioned, this USB will firmware-sanitise this machine&rsquo;s
          internal disks on a valid distress trigger. Keep it secured.
        </div>

        <label className="text-sm text-[var(--text-dim)]" htmlFor="cwu-drive-select">
          Removable drive:
        </label>{" "}
        {volumes.length === 0 ? (
          <p className="py-3 text-center text-[var(--text-mute)] text-sm">
            No removable drives detected. Insert the USB and reopen this dialog.
          </p>
        ) : (
          <select
            id="cwu-drive-select"
            className="ml-2 rounded-[var(--r-md)] border border-[var(--border)] bg-[var(--surface-1)] px-2 py-1 text-sm"
            value={selected}
            onChange={(e) => setSelected(e.target.value)}
            disabled={loading || provisioned !== null}
          >
            {volumes.map((v) => (
              <option key={v.driveLetter} value={v.driveLetter}>
                {v.driveLetter} {v.label ? `(${v.label})` : "(no label)"}
              </option>
            ))}
          </select>
        )}

        {error && (
          <div
            role="alert"
            className="mt-3 rounded-[var(--r-md)] border border-[var(--danger)]/40 bg-[var(--danger)]/10 px-3 py-2 text-xs text-[var(--danger)]"
          >
            {error}
          </div>
        )}

        {provisioned && (
          <div className="mt-3 rounded-[var(--r-md)] border border-[var(--accent-line)] bg-[var(--accent)]/10 px-3 py-2 text-xs text-[var(--text-dim)]">
            <p>
              <span className="font-semibold text-[var(--text)]">Device bound.</span>{" "}
              ID: <span className="font-mono">{provisioned.deviceId}</span>
            </p>
            <p className="font-mono mt-1 break-all">{provisioned.pubkeyPath}</p>
            <p className="font-mono break-all">{provisioned.deviceIdPath}</p>
          </div>
        )}
      </DialogBody>
      <DialogFooter
        actions={
          <>
            <Button text="Close" onClick={onClose} disabled={loading} />
            <Button
              text={loading ? "Provisioning…" : "Provision USB"}
              intent="danger"
              disabled={
                loading || volumes.length === 0 || !selected || provisioned !== null
              }
              onClick={handleProvision}
            />
          </>
        }
      />
    </Dialog>
  );
}
