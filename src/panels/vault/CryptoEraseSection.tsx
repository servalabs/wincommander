// Secure-persona "Crypto-Erase" surface inside the vault panel. Lists eligible
// encrypted containers (VeraCrypt mounted + BitLocker volumes) with the method
// that will be applied and the escrow-risk / system badges, and runs a
// single-target crypto-erase behind the ceremony in CryptoEraseConfirmDialog.
// No "anti-forensic" wording. All logic lives in useCryptoErase /
// cryptoEraseTargets / cryptoEraseReceipt.
//
// BACKEND GAP (frontend cannot fix): `erase_encrypted_container` is one
// request/response Tauri command with no progress events, so the dialog can only
// report the two client-observable phases (destroy, then re-read). Per-step
// progress would need the command to emit events.
import { useCallback, useState } from "react";
import { Button, Callout, Icon, Popover, Spinner } from "@/components/ui/bp";
import { open } from "@tauri-apps/plugin-dialog";
import EmptyState from "../../components/shared/EmptyState";
import { useCryptoErase } from "../../hooks/useCryptoErase";
import { isVeraCryptDevicePath, type EncryptedTarget } from "../../lib/cryptoEraseTargets";
import { showError, showSuccess } from "../../utils/toast";
import CryptoEraseConfirmDialog from "./CryptoEraseConfirmDialog";
import CryptoEraseReceiptPanel from "./CryptoEraseReceiptPanel";
import CryptoEraseTargetRow from "./CryptoEraseTargetRow";
import "./CryptoEraseSection.css";

type VeraVolume = { letter: string; path: string | null; type: string };

function HowItWorks() {
  return (
    <Popover
      position="bottom-end"
      content={
        <div className="crypto-erase-explainer">
          <strong>What crypto-erase does</strong>
          <p>
            An encrypted volume is unreadable without its key. Crypto-erase destroys the key instead
            of the data: seconds rather than hours, no overwriting, and it works on SSDs — where
            overwriting is unreliable because the controller quietly keeps copies in spare blocks.
          </p>
          <strong>The two methods differ</strong>
          <p>
            <em>VeraCrypt</em> — the volume header holds the only copy of the master key. It gets
            overwritten with random bytes, so the password can never derive the key again.
            <br />
            <em>BitLocker</em> — every key protector (TPM, PIN, recovery key) is removed and the
            volume is locked so Windows drops its key from memory.
          </p>
          <strong>Escrow risk</strong>
          <p>
            If a BitLocker recovery key was ever backed up to Entra/Active Directory or a Microsoft
            account, that copy survives everything done here. The volume stays recoverable until you
            delete the saved key there too. A row marked <em>escrow risk</em> is telling you exactly
            that, and the receipt will report{" "}
            <em>erased — but recovery is still possible</em> rather than claiming success.
          </p>
          <strong>Receipt statuses</strong>
          <p>
            <em>Keys destroyed</em> — verified, nothing left to unlock it.
            <br />
            <em>Recovery still possible</em> — the destruction ran, but an escrowed key or a key
            still in memory means the data is not yet unrecoverable.
            <br />
            <em>Failed</em> — nothing was destroyed.
          </p>
        </div>
      }
    >
      <Button minimal small icon="info-sign" text="How it works" />
    </Popover>
  );
}

export default function CryptoEraseSection({ veracryptVolumes }: { veracryptVolumes: VeraVolume[] }) {
  const { targets, systemDrive, loading, refreshing, loadError, receipts, history, refresh, erase } =
    useCryptoErase(veracryptVolumes);
  const [active, setActive] = useState<EncryptedTarget | null>(null);
  const [adhoc, setAdhoc] = useState<EncryptedTarget[]>([]);

  const eraseByPath = useCallback(async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "Container File", extensions: ["hc", "tc", "*"] }],
    });
    if (selected && typeof selected === "string") {
      const target: EncryptedTarget = {
        id: `vcfile:${selected}`,
        kind: "veracrypt",
        label: `VeraCrypt container ${selected}`,
        path: selected,
        isOsVolume: isVeraCryptDevicePath(selected),
        eligible: true,
      };
      // Pin it into the list up front so its receipt has somewhere to live once
      // the dialog closes, whatever the outcome.
      setAdhoc((prev) => (prev.some((a) => a.id === target.id) ? prev : [...prev, target]));
      setActive(target);
    }
  }, []);

  // The dialog owns the visible result; the notification bell keeps the record,
  // so the operation still shows up in the app's notification history.
  const eraseAndNotify = useCallback<typeof erase>(
    async (target, osAck, onPhase) => {
      const receipt = await erase(target, osAck, onPhase);
      if (receipt.status === "erased") showSuccess(`${receipt.label}: crypto-erased.`);
      else if (receipt.status === "erased_with_caveat")
        showError(`${receipt.label}: erased, but a recovery key may survive.`);
      else showError(`${receipt.label}: erase failed.`);
      return receipt;
    },
    [erase],
  );

  const displayTargets = [...targets, ...adhoc.filter((a) => !targets.some((t) => t.id === a.id))];
  // Receipts whose target has since dropped out of the volume list — a BitLocker
  // volume can vanish from Get-BitLockerVolumes the moment its protectors go.
  const orphanRecords = history.filter(
    (record) => !displayTargets.some((t) => t.id === record.targetId),
  );

  return (
    <div className="crypto-erase-section">
      <div className="crypto-erase-header">
        <div className="vault-card-icon">
          <Icon icon="key" size={16} />
        </div>
        <div className="vault-card-title-area">
          <h3>Crypto-Erase</h3>
          <span>Destroy an encrypted container's keys — permanent, in place, no reboot</span>
        </div>
        <HowItWorks />
        <Button minimal small icon="folder-open" text="Erase container file…" onClick={eraseByPath} />
        <Button
          minimal
          small
          icon="refresh"
          loading={refreshing}
          onClick={refresh}
          aria-label="Refresh the encrypted-volume list"
          title="Refresh the encrypted-volume list"
        />
      </div>

      {loadError && (
        <Callout intent="warning" title="Couldn't read the BitLocker volume list">
          {loadError} VeraCrypt volumes are still listed below, and you can always erase a container
          by picking its file.
        </Callout>
      )}

      {loading ? (
        <div className="crypto-erase-loading">
          <Spinner size={16} />
          Looking for encrypted volumes…
        </div>
      ) : displayTargets.length === 0 ? (
        <EmptyState
          icon="key"
          title="No encrypted containers detected"
          hint="Mount a VeraCrypt volume or enable BitLocker to see it here. An unmounted container can still be erased directly from its file."
          action={
            <Button minimal small icon="folder-open" text="Erase container file…" onClick={eraseByPath} />
          }
        />
      ) : (
        <div className="crypto-erase-list">
          {displayTargets.map((target) => (
            <CryptoEraseTargetRow
              key={target.id}
              target={target}
              record={receipts[target.id]}
              busy={active !== null}
              onErase={setActive}
            />
          ))}
        </div>
      )}

      {orphanRecords.length > 0 && (
        <div className="crypto-erase-history">
          <span className="crypto-erase-history-title">Earlier results</span>
          {orphanRecords.map((record) => (
            <CryptoEraseReceiptPanel
              key={`${record.targetId}-${record.at}`}
              receipt={record.receipt}
              at={record.at}
            />
          ))}
        </div>
      )}

      <CryptoEraseConfirmDialog
        target={active}
        systemDrive={systemDrive}
        onClose={() => setActive(null)}
        onErase={eraseAndNotify}
      />
    </div>
  );
}
