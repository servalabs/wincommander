// One crypto-erase candidate. Shows the kind, the method that will be applied
// and the escrow/system badges with explanations — the badges used to be bare
// 10px uppercase text with no way to find out what they meant, and "escrow risk"
// is the difference between this feature working and not working.
import { Button, Icon, Tooltip } from "@/components/ui/bp";
import { eraseMethodLabel, targetSubject, type EncryptedTarget } from "../../lib/cryptoEraseTargets";
import type { EraseRecord } from "../../hooks/useCryptoErase";
import CryptoEraseReceiptPanel from "./CryptoEraseReceiptPanel";

const SYSTEM_TIP =
  "This is the volume Windows boots from. Erasing its keys stops this machine from starting, so it needs a typed confirmation.";
const ESCROW_TIP =
  "A recovery key for this volume is backed up somewhere else — Entra/Active Directory, or a Microsoft account. Anyone with that key can still unlock the volume after the erase, so it has to be revoked there too.";

export default function CryptoEraseTargetRow({
  target,
  record,
  busy,
  onErase,
}: {
  target: EncryptedTarget;
  record?: EraseRecord;
  busy: boolean;
  onErase: (target: EncryptedTarget) => void;
}) {
  // A target with a non-failed receipt has nothing left to destroy. Keeping the
  // button live invited a second, pointless erase of an already-dead container.
  const alreadyErased = record != null && record.receipt.status !== "failed";
  const actionable = target.eligible && !alreadyErased;

  return (
    <div className={`crypto-erase-row${actionable ? "" : " is-ineligible"}`}>
      <div className="crypto-erase-row-main">
        <span className="crypto-erase-row-label">
          <span className="crypto-erase-kind">
            {target.kind === "veracrypt" ? "VeraCrypt" : "BitLocker"}
          </span>
          <span className="crypto-erase-row-name" title={targetSubject(target)}>
            {targetSubject(target)}
          </span>
          {target.isOsVolume && (
            <Tooltip content={SYSTEM_TIP}>
              <span className="crypto-erase-badge crypto-erase-badge--os">
                SYSTEM
                <Icon icon="info-sign" size={9} />
              </span>
            </Tooltip>
          )}
          {target.escrowRisk && (
            <Tooltip content={ESCROW_TIP}>
              <span className="crypto-erase-badge crypto-erase-badge--escrow">
                ESCROW RISK
                <Icon icon="info-sign" size={9} />
              </span>
            </Tooltip>
          )}
        </span>

        <span className="crypto-erase-row-method">{eraseMethodLabel(target)}</span>
        {target.reason && <span className="crypto-erase-row-reason">{target.reason}</span>}
        {record && (
          <CryptoEraseReceiptPanel receipt={record.receipt} at={record.at} compact />
        )}
      </div>

      {alreadyErased ? (
        <span className="crypto-erase-row-done">
          <Icon icon="tick-circle" size={13} />
          Erased
        </span>
      ) : (
        <Button
          small
          intent="danger"
          icon="trash"
          text="Crypto-Erase"
          disabled={!target.eligible || busy}
          onClick={() => onErase(target)}
        />
      )}
    </div>
  );
}
