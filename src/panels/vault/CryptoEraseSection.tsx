// Secure-persona "Crypto-Erase" surface inside the vault panel. Lists eligible
// encrypted containers (VeraCrypt mounted + BitLocker volumes), shows the
// BitLocker escrow-risk badge, and runs a single-target crypto-erase behind a
// confirm ceremony. OS/system volumes escalate to a typed "won't boot" gate.
// No "anti-forensic" wording. All logic lives in useCryptoErase / cryptoEraseTargets.
import { useState, useCallback } from "react";
import { Button, Checkbox, Dialog, Icon, InputGroup, Spinner } from "@/components/ui/bp";
import { open } from "@tauri-apps/plugin-dialog";
import { useCryptoErase } from "../../hooks/useCryptoErase";
import { requiresNuclear, isVeraCryptDevicePath, type EncryptedTarget } from "../../lib/cryptoEraseTargets";
import { showSuccess, showError } from "../../utils/toast";
import "./CryptoEraseSection.css";

type VeraVolume = { letter: string; path: string | null; type: string };

export default function CryptoEraseSection({ veracryptVolumes }: { veracryptVolumes: VeraVolume[] }) {
  const { targets, loading, refreshing, receipts, refresh, erase } = useCryptoErase(veracryptVolumes);
  const [active, setActive] = useState<EncryptedTarget | null>(null);
  const [ack, setAck] = useState(false);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [adhoc, setAdhoc] = useState<EncryptedTarget[]>([]);

  const nuclear = active ? requiresNuclear(active) : false;
  const resolvedId = active?.mountPoint ? active.mountPoint.replace(/\\+$/, "").toUpperCase() : "C:";
  const canFire = active
    ? nuclear
      ? typed.trim().toUpperCase() === resolvedId
      : ack
    : false;

  const openConfirm = useCallback((t: EncryptedTarget) => {
    setActive(t);
    setAck(false);
    setTyped("");
  }, []);

  const closeConfirm = useCallback(() => {
    setActive(null);
    setAck(false);
    setTyped("");
  }, []);

  const eraseByPath = useCallback(async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "Container File", extensions: ["hc", "tc", "*"] }],
    });
    if (selected && typeof selected === "string") {
      openConfirm({
        id: `vcfile:${selected}`,
        kind: "veracrypt",
        label: `VeraCrypt container ${selected}`,
        path: selected,
        isOsVolume: isVeraCryptDevicePath(selected),
        eligible: true,
      });
    }
  }, [openConfirm]);

  const confirmErase = useCallback(async () => {
    if (!active) return;
    setBusy(true);
    try {
      const receipt = await erase(active, nuclear ? typed.trim() : undefined);
      if (active.id.startsWith("vcfile:")) {
        setAdhoc((prev) => (prev.some((a) => a.id === active.id) ? prev : [...prev, active]));
      }
      if (receipt) {
        if (receipt.status === "erased") showSuccess(`${receipt.label}: crypto-erased.`);
        else if (receipt.status === "erased_with_caveat") showError(`${receipt.label}: erased, but a recovery key may survive.`);
        else showError(`${receipt.label}: erase failed.`);
      }
      closeConfirm();
    } catch (e) {
      showError(e instanceof Error ? e.message : "Crypto-erase failed");
    } finally {
      setBusy(false);
    }
  }, [active, nuclear, typed, erase, closeConfirm]);

  const displayTargets = [...targets, ...adhoc.filter((a) => !targets.some((t) => t.id === a.id))];

  return (
    <div className="crypto-erase-section">
      <div className="crypto-erase-header">
        <div className="vault-card-icon"><Icon icon="key" size={16} /></div>
        <div className="vault-card-title-area">
          <h3>Crypto-Erase</h3>
          <span>Destroy an encrypted container's keys — permanent, in place, no reboot</span>
        </div>
        <Button minimal small icon="folder-open" text="Erase container file…" onClick={eraseByPath} />
        <Button minimal small icon="refresh" loading={refreshing} onClick={refresh} aria-label="Refresh" />
      </div>

      {loading ? (
        <div className="crypto-erase-empty"><Spinner size={20} /></div>
      ) : displayTargets.length === 0 ? (
        <div className="crypto-erase-empty">No encrypted containers detected.</div>
      ) : (
        <div className="crypto-erase-list">
          {displayTargets.map((t) => {
            const r = receipts[t.id];
            return (
              <div key={t.id} className={`crypto-erase-row${t.eligible ? "" : " is-ineligible"}`}>
                <div className="crypto-erase-row-main">
                  <span className="crypto-erase-row-label">
                    {t.label}
                    {t.isOsVolume && <span className="crypto-erase-badge crypto-erase-badge--os">SYSTEM</span>}
                    {t.escrowRisk && <span className="crypto-erase-badge crypto-erase-badge--escrow">escrow risk</span>}
                  </span>
                  {t.reason && <span className="crypto-erase-row-reason">{t.reason}</span>}
                  {r && <span className={`crypto-erase-receipt crypto-erase-receipt--${r.status}`}>{r.detail}</span>}
                </div>
                <Button
                  small
                  intent="danger"
                  icon="trash"
                  text="Crypto-Erase"
                  disabled={!t.eligible}
                  onClick={() => openConfirm(t)}
                />
              </div>
            );
          })}
        </div>
      )}

      <Dialog isOpen={!!active} onClose={closeConfirm} title={nuclear ? "Erase system volume" : "Crypto-erase volume"}>
        <div className="wc-dialog-body crypto-erase-confirm">
          {nuclear ? (
            <>
              <p className="crypto-erase-warn crypto-erase-warn--nuclear">
                This will remove the keys for <strong>{active?.label}</strong>. THIS MACHINE WILL NOT BOOT
                AFTER RESTART. Type <strong>{resolvedId}</strong> to confirm.
              </p>
              <InputGroup value={typed} autoComplete="off" placeholder={resolvedId} onChange={(e) => setTyped(e.target.value)} />
            </>
          ) : (
            <>
              <p className="crypto-erase-warn">
                Permanently destroy the encryption keys for <strong>{active?.label}</strong>. The data cannot
                be recovered. The rest of the system is unaffected.
              </p>
              <Checkbox checked={ack} onChange={() => setAck((v) => !v)} label="I understand this is irreversible" />
            </>
          )}
        </div>
        <div className="mount-dialog-footer">
          <Button minimal icon="cross" text="CANCEL" onClick={closeConfirm} />
          <Button intent="danger" icon="trash" text="CRYPTO-ERASE" loading={busy} disabled={!canFire || busy} onClick={confirmErase} />
        </div>
      </Dialog>
    </div>
  );
}
