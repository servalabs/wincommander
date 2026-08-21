import { useEffect, useRef, useState, type PointerEvent } from "react";
import {
  AlertDialog,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "../ui/alert-dialog";
import { Button } from "../ui/button";
import { useUsbHidApproval } from "../../context/UsbHidApprovalContext";
import {
  canApproveWithPointer,
  deviceFingerprint,
  deviceLabel,
  isValidVisualChallenge,
  isVisualChallengeFollowUp,
  isVisualChallengeLocked,
  isVisualChallengeProgress,
  type UsbHidVisualChallenge,
} from "../../lib/usbHidApproval";

interface PointerActionButtonProps {
  action: () => void;
  busy: boolean;
  children: string;
  variant?: "default" | "primary" | "danger";
  className?: string;
}

/**
 * This makes keyboard activation a no-op. It is deliberately not presented as
 * proof of physical mouse identity: a hostile composite device can synthesize
 * browser input. The challenge is only a visual human-presence speed bump.
 */
function PointerActionButton({
  action,
  busy,
  children,
  variant = "default",
  className,
}: PointerActionButtonProps) {
  const pointerId = useRef<number | null>(null);

  const armPointer = (event: PointerEvent<HTMLButtonElement>) => {
    pointerId.current = event.isTrusted && event.isPrimary && canApproveWithPointer(event.pointerType, event.button)
      ? event.pointerId
      : null;
  };
  const activate = (event: PointerEvent<HTMLButtonElement>) => {
    const armed = pointerId.current === event.pointerId;
    pointerId.current = null;
    event.preventDefault();
    if (
      busy
      || !armed
      || !event.isTrusted
      || !event.isPrimary
      || !canApproveWithPointer(event.pointerType, event.button)
    ) return;
    action();
  };

  return (
    <Button
      type="button"
      variant={variant}
      disabled={busy}
      tabIndex={-1}
      className={className}
      onPointerDown={armPointer}
      onPointerUp={activate}
      onPointerCancel={() => { pointerId.current = null; }}
      onClick={(event) => {
        // detail=0 is the browser's keyboard/programmatic activation shape.
        // Prevent every click too: pointerup above is the sole action path.
        if (event.detail === 0) event.preventDefault();
        event.preventDefault();
      }}
      onKeyDown={(event) => event.preventDefault()}
      onKeyUp={(event) => event.preventDefault()}
    >
      {children}
    </Button>
  );
}

interface ChallengeSession {
  challenge: UsbHidVisualChallenge;
}

function newSession(challenge: UsbHidVisualChallenge): ChallengeSession | null {
  if (
    challenge.step < 1
    || challenge.totalSteps < challenge.step
    || !isValidVisualChallenge(challenge)
  ) return null;
  return { challenge };
}

export default function UsbHidApprovalDialog() {
  const {
    pending,
    beginVisualChallenge,
    submitVisualChallengeDigit,
    keepBlocked,
  } = useUsbHidApproval();
  const [confirmingTrust, setConfirmingTrust] = useState(false);
  const [session, setSession] = useState<ChallengeSession | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const current = pending[0];
  const canAllowOnce = current?.availableActions?.includes("allowOnce") === true;
  const canTrustAlways = current?.availableActions?.includes("trustAlways") === true;
  const isContained = current?.enforcement.succeeded === true && canAllowOnce;

  useEffect(() => {
    setConfirmingTrust(false);
    setSession(null);
    setError(null);
  }, [current?.deviceKey]);

  const beginChallenge = (action: UsbHidVisualChallenge["action"]) => {
    if (!current || !isContained || busy) return;
    setBusy(true);
    setError(null);
    void beginVisualChallenge(current.deviceKey, action)
      .then((challenge) => {
        const next = newSession(challenge);
        if (!next || challenge.deviceKey !== current.deviceKey || challenge.action !== action) {
          throw new Error("The approval challenge was invalid. The device remains blocked.");
        }
        setSession(next);
      })
      .catch((reason) => setError(String(reason)))
      .finally(() => setBusy(false));
  };

  const retryOrKeepBlocked = () => {
    if (!current || busy) return;
    setBusy(true);
    setError(null);
    void keepBlocked(current.deviceKey)
      .catch((reason) => setError(String(reason)))
      .finally(() => setBusy(false));
  };

  const chooseDigit = (digit: number) => {
    if (!current || !session || busy) return;
    setBusy(true);
    setError(null);
    void submitVisualChallengeDigit(
      current.deviceKey,
      session.challenge.challengeId,
      session.challenge.step,
      String(digit),
    )
      .then((result) => {
        if (isVisualChallengeLocked(result)) {
          setSession(null);
          setError(`Too many incorrect attempts. This keyboard remains blocked until ${new Date(result.retryAfterEpoch * 1000).toLocaleTimeString()}.`);
          return;
        }
        if (isVisualChallengeProgress(result)) {
          const next = newSession(result.challenge);
          if (!next || result.challenge.challengeId !== session.challenge.challengeId) {
            throw new Error("The approval progress response was invalid.");
          }
          setSession(next);
          return;
        }
        if (!isVisualChallengeFollowUp(result)) {
          setSession(null);
          setConfirmingTrust(false);
          return;
        }
        const next = newSession(result.challenge);
        const sameStepReplacement = result.challenge.action === session.challenge.action
          && result.challenge.step === session.challenge.step
          && result.challenge.totalSteps === session.challenge.totalSteps;
        const trustSecondStep = session.challenge.action === "trustAlways"
          && session.challenge.step === 1
          && result.challenge.action === "trustAlways"
          && result.challenge.step === 2
          && result.challenge.totalSteps === 2;
        if (
          !next
          || result.challenge.deviceKey !== current.deviceKey
          || (!sameStepReplacement && !trustSecondStep)
        ) throw new Error("The replacement approval challenge was invalid.");
        setSession(next);
        if (sameStepReplacement) {
          setError("That digit did not match. Pro rotated the challenge and kept the keyboard blocked.");
        }
      })
      .catch((reason) => {
        setSession(null);
        setError(`The challenge was not accepted. ${String(reason)}`);
      })
      .finally(() => setBusy(false));
  };

  const fingerprint = current ? deviceFingerprint(current) : null;
  const challengeText = session?.challenge.displaySequence.split("").join(" ");
  const challengeStep = session
    ? `Challenge ${session.challenge.step} of ${session.challenge.totalSteps}`
    : null;

  return (
    <AlertDialog open={current !== undefined} onOpenChange={() => {}}>
      <AlertDialogContent
        onEscapeKeyDown={(event) => event.preventDefault()}
      >
        <AlertDialogHeader>
          <AlertDialogTitle>New input device</AlertDialogTitle>
          <AlertDialogDescription className="space-y-2">
            <span className="block">
              {current ? deviceLabel(current) : "A new keyboard or input device"} is blocked pending your choice.
            </span>
            {current?.manufacturer && <span className="block">Manufacturer: {current.manufacturer}</span>}
            {fingerprint && <span className="block font-mono">Device: {fingerprint}</span>}
            <span className="block">
              This is reactive after Windows detects the device. It does not claim to stop a first keystroke or pre-boot input.
            </span>
            {isContained ? (
              <span className="block">No choice means it remains blocked when the approval window expires.</span>
            ) : (
              <span className="block text-[var(--danger)]">
                Securing this device did not succeed. Unplug it now, then retry blocking. Do not rely on the timeout for containment.
              </span>
            )}
            {!isContained && current?.enforcement.error && (
              <span className="block text-[var(--danger)]">Containment detail: {current.enforcement.error}</span>
            )}
            {current?.topologyWarning && (
              <span className="block text-[var(--danger)]">Topology detail: {current.topologyWarning}</span>
            )}
            {session && (
              <span className="block rounded-[var(--r)] border border-[var(--warn)]/40 bg-[var(--surface-2)] p-2 text-[var(--text)]">
                {challengeStep}: visually match <strong className="font-mono tracking-[0.2em]">{challengeText}</strong>. The keypad moves after every click.
              </span>
            )}
            {session && (
              <span className="block text-xs text-[var(--text-mute)]">
                Visual click confirmation is defense in depth; it does not attest to the identity of a physical mouse.
              </span>
            )}
            {error && <span className="block text-[var(--danger)]">{error}</span>}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter className="flex-wrap">
          {!isContained ? (
            <PointerActionButton busy={busy} variant="danger" action={retryOrKeepBlocked}>
              Retry block
            </PointerActionButton>
          ) : session ? (
            <div className="grid w-full grid-cols-5 gap-2" aria-label="Visual confirmation keypad">
              {session.challenge.keypadLayout.map((digit) => (
                <PointerActionButton
                  key={digit}
                  busy={busy}
                  className="h-10 px-0 font-mono"
                  action={() => chooseDigit(Number(digit))}
                >
                  {String(digit)}
                </PointerActionButton>
              ))}
            </div>
          ) : confirmingTrust ? (
            <>
              <span className="mr-auto max-w-xs text-xs text-[var(--warn)]">
                Always trust requires two fresh visual confirmation challenges before this stable-serial device is remembered.
              </span>
              <PointerActionButton busy={busy} action={() => setConfirmingTrust(false)}>
                Cancel
              </PointerActionButton>
              <PointerActionButton busy={busy} variant="primary" action={() => beginChallenge("trustAlways")}>
                Begin always-trust challenge
              </PointerActionButton>
            </>
          ) : (
            <>
              <PointerActionButton busy={busy} variant="danger" action={retryOrKeepBlocked}>
                Keep blocked
              </PointerActionButton>
              <PointerActionButton busy={busy} action={() => beginChallenge("allowOnce")}>
                Allow once
              </PointerActionButton>
              {canTrustAlways ? (
                <PointerActionButton busy={busy} variant="primary" action={() => setConfirmingTrust(true)}>
                  Always trust
                </PointerActionButton>
              ) : (
                <span className="max-w-xs text-xs text-[var(--warn)]">
                  {current?.trustAlwaysWarning ?? "Always trust is unavailable because this device has no stable hardware serial."}
                </span>
              )}
            </>
          )}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
