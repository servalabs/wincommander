// The crypto-erase ceremony: confirm → running → result, in one dialog.
//
// Friction matches the house standard (ConfirmIrreversibleDialog): a named
// blast radius, a consequence list, a stated limitation, and an enforced pause
// before the action arms. The typed acknowledgement on OS/system volumes is
// kept because the SERVER requires it (selective_erase.rs) — the checkbox path
// gains the countdown it was missing.
//
// The dialog stays open through the operation and then shows the receipt: for an
// irreversible security action the proof belongs in front of the user, not in a
// notification bell they have to go and open.
import { useEffect, useRef, useState } from "react";
import {
  Button,
  Callout,
  Checkbox,
  Dialog,
  DialogBody,
  DialogFooter,
  Icon,
  InputGroup,
  ProgressBar,
} from "@/components/ui/bp";
import {
  eraseConsequences,
  eraseLimitation,
  eraseMethodLabel,
  expectedAckToken,
  requiresNuclear,
  targetSubject,
  type EncryptedTarget,
} from "../../lib/cryptoEraseTargets";
import { classifyEraseError } from "../../lib/cryptoEraseReceipt";
import type { ErasePhase } from "../../hooks/useCryptoErase";
import type { EraseReceipt } from "../../hooks/useBackend";
import CryptoEraseReceiptPanel from "./CryptoEraseReceiptPanel";
import "./CryptoEraseDialog.css";

const COUNTDOWN_SECONDS = 3;

interface Props {
  target: EncryptedTarget | null;
  systemDrive: string;
  onClose: () => void;
  onErase: (
    target: EncryptedTarget,
    osAck: string | undefined,
    onPhase: (phase: ErasePhase) => void,
  ) => Promise<EraseReceipt>;
}

function phaseLabel(phase: ErasePhase, target: EncryptedTarget): string {
  if (phase === "destroying") {
    return target.kind === "veracrypt"
      ? "Dismounting the volume and overwriting its header…"
      : "Removing every key protector…";
  }
  if (phase === "verifying") return "Re-reading the volume to verify the result…";
  return "Finishing up…";
}

export default function CryptoEraseConfirmDialog({ target, systemDrive, onClose, onErase }: Props) {
  const [ack, setAck] = useState(false);
  const [typed, setTyped] = useState("");
  const [remaining, setRemaining] = useState(COUNTDOWN_SECONDS);
  const [phase, setPhase] = useState<ErasePhase | null>(null);
  const [receipt, setReceipt] = useState<{ receipt: EraseReceipt; at: number } | null>(null);
  const [error, setError] = useState<{ title: string; hint: string } | null>(null);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const isOpen = target !== null;
  const nuclear = target ? requiresNuclear(target) : false;
  const ackToken = target ? expectedAckToken(target, systemDrive) : "";
  // Stays true through the "done" phase until the receipt lands, so the dialog
  // never flashes a re-armed Cancel/Erase pair between the two state updates.
  const running = phase !== null && receipt === null;

  // Reset the whole ceremony each time a target is chosen — a stale countdown
  // or a leftover receipt from the previous target must never carry over.
  useEffect(() => {
    if (!isOpen) return;
    setAck(false);
    setTyped("");
    setPhase(null);
    setReceipt(null);
    setError(null);
    setRemaining(COUNTDOWN_SECONDS);
    timerRef.current = setInterval(() => setRemaining((r) => Math.max(0, r - 1)), 1000);
    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
      timerRef.current = null;
    };
  }, [isOpen, target?.id]);

  const gatePassed = nuclear ? typed.trim().toUpperCase() === ackToken : ack;
  const canFire = gatePassed && remaining <= 0 && !running;

  const fire = async () => {
    if (!target || !canFire) return;
    setError(null);
    try {
      const result = await onErase(target, nuclear ? typed.trim() : undefined, setPhase);
      setReceipt({ receipt: result, at: Date.now() });
    } catch (cause) {
      setPhase(null);
      setError(classifyEraseError(cause instanceof Error ? cause.message : String(cause)));
    }
  };

  const title = receipt
    ? "Crypto-erase result"
    : nuclear
      ? "Erase the volume Windows boots from"
      : "Crypto-erase this volume";

  return (
    <Dialog
      isOpen={isOpen}
      onClose={running ? () => {} : onClose}
      title={title}
      icon={receipt ? "key" : "warning-sign"}
      canEscapeKeyClose={!running}
      canOutsideClickClose={!running}
      isCloseButtonShown={!running}
    >
      <DialogBody className="crypto-erase-confirm">
        {!target ? null : receipt ? (
          <CryptoEraseReceiptPanel receipt={receipt.receipt} at={receipt.at} />
        ) : (
          <>
            <div className="crypto-erase-subject">
              <span className="crypto-erase-subject-label">Target</span>
              <span className="crypto-erase-subject-value" title={targetSubject(target)}>
                {targetSubject(target)}
              </span>
              <span className="crypto-erase-subject-method">{eraseMethodLabel(target)}</span>
            </div>

            <div>
              <p className="crypto-erase-lede">
                This <strong>cannot be undone.</strong> The following will happen:
              </p>
              <ul className="crypto-erase-consequences">
                {eraseConsequences(target).map((line) => (
                  <li key={line}>{line}</li>
                ))}
              </ul>
            </div>

            <Callout intent="warning" title="What this does and does not guarantee">
              {eraseLimitation(target)}
            </Callout>

            {nuclear ? (
              <div className="crypto-erase-gate">
                <p className="crypto-erase-warn--nuclear">
                  <Icon icon="warning-sign" size={13} /> This machine will not start after a restart.
                </p>
                <label className="crypto-erase-gate-label" htmlFor="crypto-erase-ack">
                  Type <strong>{ackToken}</strong> to confirm
                </label>
                <InputGroup
                  id="crypto-erase-ack"
                  value={typed}
                  autoComplete="off"
                  placeholder={ackToken}
                  disabled={running}
                  onChange={(e) => setTyped(e.target.value)}
                />
                <span className="crypto-erase-gate-hint">
                  {ackToken} is the drive Windows boots from — the same token the erase engine checks
                  before it will touch a system volume.
                </span>
              </div>
            ) : (
              <div className="crypto-erase-gate">
                <Checkbox
                  checked={ack}
                  disabled={running}
                  onChange={() => setAck((v) => !v)}
                  label="I understand this is irreversible"
                />
              </div>
            )}

            {running && (
              <div className="crypto-erase-progress">
                <ProgressBar intent="danger" />
                <span className="crypto-erase-progress-label">
                  {phase ? phaseLabel(phase, target) : ""}
                </span>
                <span className="crypto-erase-gate-hint">
                  Leave this window open until it finishes.
                </span>
              </div>
            )}

            {error && (
              <Callout intent="danger" title={error.title}>
                {error.hint}
              </Callout>
            )}
          </>
        )}
      </DialogBody>

      <DialogFooter>
        {receipt ? (
          <Button intent="primary" text="DONE" onClick={onClose} />
        ) : (
          <>
            <Button minimal icon="cross" text="CANCEL" disabled={running} onClick={onClose} />
            <Button
              intent="danger"
              icon="trash"
              loading={running}
              disabled={!canFire}
              onClick={fire}
              text={
                running
                  ? "ERASING…"
                  : remaining > 0
                    ? `CRYPTO-ERASE (${remaining}s)`
                    : error
                      ? "RETRY CRYPTO-ERASE"
                      : "CRYPTO-ERASE"
              }
            />
          </>
        )}
      </DialogFooter>
    </Dialog>
  );
}
