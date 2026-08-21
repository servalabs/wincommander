// src/panels/privacy/VerifyUsbBootDialog.tsx
//
// F6 USB boot chain self-test (P1 Piece 3). Lets an operator verify the full
// reboot -> SystemRescue -> verify-token.sh -> consumed-nonces loop actually
// works, without going through a real duress trigger.
//
// DANGER: "Arm" writes a REAL, validly-signed wipe token to the USB and sets
// a REAL one-shot UEFI BootNext entry. The wire format has no "test mode" --
// if the armed machine reboots for ANY reason before the token expires, it
// really wipes. This dialog never reboots anything itself (the operator
// reboots manually) and requires typing an explicit confirmation phrase
// before the Arm button is even enabled -- see f6_verify_boot.rs's module
// header for the full backend-side contract this mirrors.
//
// Uses direct invoke() (NOT routed through useBackend.ts), matching
// CreateWipeUsbDialog.tsx's pattern for the same feature family.

import { useCallback, useEffect, useState } from "react";
import { Button, Dialog, DialogBody, DialogFooter } from "@/components/ui/bp";
import { showError, showSuccess } from "../../utils/toast";
import {
  verifyUsbBoot,
  type VerifyUsbBootArmResult as ArmResult,
  type VerifyUsbBootCheckResult as CheckResult,
} from "../../hooks/useVerifyUsbBoot";

const CONFIRM_PHRASE = "THIS IS A DISPOSABLE TEST MACHINE";

interface Props {
  open: boolean;
  onClose: () => void;
}

type Stage = "confirm" | "armed" | "checked";

export default function VerifyUsbBootDialog({ open, onClose }: Props) {
  const [stage, setStage] = useState<Stage>("confirm");
  const [confirmText, setConfirmText] = useState("");
  const [armResult, setArmResult] = useState<ArmResult | null>(null);
  const [checkResult, setCheckResult] = useState<CheckResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [remainingSecs, setRemainingSecs] = useState<number | null>(null);

  useEffect(() => {
    if (!open) {
      setStage("confirm");
      setConfirmText("");
      setArmResult(null);
      setCheckResult(null);
      setError(null);
      setRemainingSecs(null);
      return;
    }
    // Rehydrate from the durable backend marker. A self-test armed before a
    // real reboot (or before the app was closed/crashed) has no surviving
    // React state -- but the marker file does. Without this, "Check result"
    // would be permanently unreachable after the one thing this tool exists
    // to validate: an actual reboot.
    verifyUsbBoot.status()
      .then((status) => {
        if (status.armed && status.usbRoot && status.nonceHex) {
          setArmResult({
            usbRoot: status.usbRoot,
            bootEntryId: status.bootEntryId ?? "",
            nonceHex: status.nonceHex,
            expiresAtUnix: status.expiresAtUnix ?? 0,
            warning:
              "A self-test was left armed from a previous session (app restart, " +
              "real reboot, or the dialog was closed without disarming). Reboot " +
              "manually to continue, check the result, or disarm to cancel.",
          });
          setStage("armed");
        }
      })
      .catch(() => {
        // Non-fatal -- just means we can't rehydrate; a fresh Arm still works.
      });
  }, [open]);

  // Live countdown to token expiry while armed -- makes the exposure
  // window visible instead of an abstract "expiresAtUnix" number.
  useEffect(() => {
    if (stage !== "armed" || !armResult) return;
    const tick = () => {
      const left = armResult.expiresAtUnix - Math.floor(Date.now() / 1000);
      setRemainingSecs(left);
    };
    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, [stage, armResult]);

  const handleArm = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await verifyUsbBoot.arm();
      setArmResult(result);
      setStage("armed");
      showSuccess("Armed. Reboot this machine manually to continue the test.");
    } catch (e: unknown) {
      const msg = String(e);
      setError(msg);
      showError(`Arm failed: ${msg}`);
    } finally {
      setLoading(false);
    }
  }, []);

  const handleDisarm = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      await verifyUsbBoot.disarm();
      showSuccess("Disarmed — BootNext cleared, token invalidated.");
      setStage("confirm");
      setConfirmText("");
      setArmResult(null);
    } catch (e: unknown) {
      const msg = String(e);
      setError(msg);
      showError(`Disarm failed: ${msg}`);
    } finally {
      setLoading(false);
    }
  }, []);

  const handleCheck = useCallback(async () => {
    if (!armResult) return;
    setLoading(true);
    setError(null);
    try {
      const result = await verifyUsbBoot.check(armResult.usbRoot, armResult.nonceHex);
      setCheckResult(result);
      setStage("checked");
      if (result.consumed) {
        showSuccess("PASS — the USB accepted the token and recorded the nonce.");
      } else {
        showError("FAIL — nonce not found in consumed-nonces. See details below.");
      }
    } catch (e: unknown) {
      const msg = String(e);
      setError(msg);
      showError(`Check failed: ${msg}`);
    } finally {
      setLoading(false);
    }
  }, [armResult]);

  const confirmMatches = confirmText.trim() === CONFIRM_PHRASE;

  return (
    <Dialog
      isOpen={open}
      onClose={onClose}
      title="Verify USB Boot — Self-Test"
      icon="warning-sign"
      className="mount-dialog"
      isCloseButtonShown={stage !== "armed"}
      canEscapeKeyClose={!loading && stage !== "armed"}
      canOutsideClickClose={!loading && stage !== "armed"}
    >
      <DialogBody>
        {stage === "confirm" && (
          <>
            <div className="mb-4 rounded-[var(--r-lg)] border border-[var(--danger)]/40 bg-[var(--danger)]/10 px-3 py-2.5 text-xs text-[var(--text-dim)] leading-relaxed">
              <span className="font-semibold text-[var(--danger)]">
                This arms a REAL wipe trigger.
              </span>{" "}
              Arming writes a validly-signed wipe token with a live ~5-minute
              window and sets this machine&rsquo;s one-shot boot entry to the
              USB. There is no &ldquo;test mode&rdquo; in the wire format — if{" "}
              <strong>this machine</strong> reboots for any reason (Windows
              Update, power loss, an unrelated manual reboot) before the token
              expires, it will really wipe. Only run this against a
              disposable machine — a Proxmox/VM with a throwaway disk — never
              a production device.
            </div>
            <label
              className="text-sm text-[var(--text-dim)] block mb-1.5"
              htmlFor="vub-confirm-input"
            >
              Type <code className="font-mono">{CONFIRM_PHRASE}</code> to
              continue:
            </label>
            <input
              id="vub-confirm-input"
              type="text"
              className="w-full rounded-[var(--r-md)] border border-[var(--border)] bg-[var(--surface-1)] px-2 py-1.5 text-sm font-mono"
              value={confirmText}
              onChange={(e) => setConfirmText(e.target.value)}
              placeholder={CONFIRM_PHRASE}
              autoComplete="off"
              spellCheck={false}
            />
          </>
        )}

        {stage === "armed" && armResult && (
          <>
            <div className="mb-3 rounded-[var(--r-lg)] border border-[var(--danger)]/40 bg-[var(--danger)]/10 px-3 py-2.5 text-xs text-[var(--text-dim)] leading-relaxed">
              <span className="font-semibold text-[var(--danger)]">
                ARMED — real wipe token live.
              </span>{" "}
              {armResult.warning}
            </div>
            <div className="mb-3 rounded-[var(--r-md)] border border-[var(--border)] bg-[var(--surface-2)]/40 px-3 py-2 text-xs text-[var(--text-dim)] font-mono leading-relaxed">
              <p>USB: {armResult.usbRoot}</p>
              <p>Boot entry: {armResult.bootEntryId}</p>
              <p>Nonce: {armResult.nonceHex.slice(0, 16)}…</p>
              <p>
                Expires in:{" "}
                <span
                  className={
                    remainingSecs !== null && remainingSecs <= 30
                      ? "text-[var(--danger)] font-semibold"
                      : ""
                  }
                >
                  {remainingSecs !== null ? `${Math.max(0, remainingSecs)}s` : "…"}
                </span>
              </p>
            </div>
            <p className="text-[var(--text-dim)] text-sm">
              Reboot this machine manually now to continue the test, or
              disarm to cancel.
            </p>
          </>
        )}

        {stage === "checked" && checkResult && (
          <div
            className={`rounded-[var(--r-md)] border px-3 py-2.5 text-sm leading-relaxed ${
              checkResult.consumed
                ? "border-[var(--accent-line)] bg-[var(--accent)]/10"
                : "border-[var(--danger)]/40 bg-[var(--danger)]/10"
            }`}
          >
            <p className="font-semibold text-[var(--text)] mb-1">
              {checkResult.consumed ? "PASS" : "FAIL"}
            </p>
            <p className="text-[var(--text-dim)] text-xs">
              {checkResult.consumed
                ? "The USB accepted the token and recorded its nonce in consumed-nonces — the full boot chain works."
                : checkResult.reason ??
                  "The nonce was not found in consumed-nonces. The pipeline may not have run, or the USB wasn't remounted at this path."}
            </p>
            {!checkResult.consumed && checkResult.bootNextCleared && (
              <p className="text-[var(--text-dim)] text-xs mt-1.5">
                Note: BootNext was cleared as part of this check. If you
                haven&rsquo;t actually rebooted yet, this FAIL doesn&rsquo;t
                mean the pipeline is broken — it means the check ran before
                the reboot did. Arm again, then reboot, then check.
              </p>
            )}
          </div>
        )}

        {error && (
          <div
            role="alert"
            className="mt-3 rounded-[var(--r-md)] border border-[var(--danger)]/40 bg-[var(--danger)]/10 px-3 py-2 text-xs text-[var(--danger)]"
          >
            {error}
          </div>
        )}
      </DialogBody>
      <DialogFooter
        actions={
          <>
            {stage !== "armed" && (
              <Button text="Close" onClick={onClose} disabled={loading} />
            )}
            {stage === "confirm" && (
              <Button
                text={loading ? "Arming…" : "Arm self-test"}
                intent="danger"
                disabled={loading || !confirmMatches}
                onClick={handleArm}
              />
            )}
            {stage === "armed" && (
              <>
                <Button
                  text={loading ? "Disarming…" : "Disarm (cancel)"}
                  disabled={loading}
                  onClick={handleDisarm}
                />
                <Button
                  text={loading ? "Checking…" : "I've rebooted — Check result"}
                  intent="primary"
                  disabled={loading}
                  onClick={handleCheck}
                />
              </>
            )}
            {stage === "checked" && (
              <Button
                text="Run again"
                onClick={() => {
                  setStage("confirm");
                  setConfirmText("");
                  setArmResult(null);
                  setCheckResult(null);
                }}
              />
            )}
          </>
        }
      />
    </Dialog>
  );
}
