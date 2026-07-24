import { useEffect, useState } from "react";
import { AnimatePresence } from "framer-motion";
import LicenseConfetti from "./LicenseConfetti";
import { showSuccess } from "../../utils/toast";

// Mount once at the root of the app. Listens for a `license-activated-celebration`
// CustomEvent and shows the LicenseConfetti animation + a congratulations
// toast. Any activation path (Identity panel, LicenseGate, LicenseQuickPanel)
// dispatches this event after a successful activation, so
// the celebration looks identical regardless of trigger source.
//
// Event detail:
//   { message?: string;  // optional banner text
//     toast?: string;    // optional toast text (defaults to message)
//   }
export default function LicenseCelebrationListener() {
  // `nonce` bumps every time we receive an event, so even if a celebration is
  // already running, mounting LicenseConfetti with a fresh key restarts it.
  const [nonce, setNonce] = useState(0);
  const [active, setActive] = useState(false);
  const [message, setMessage] = useState<string | undefined>(undefined);

  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent<{ message?: string; toast?: string }>).detail;
      const toastText = detail?.toast ?? detail?.message ?? "WinCommander Pro is now active — all paid features unlocked. Enjoy!";
      showSuccess(`Congratulations! ${toastText}`);
      setMessage(detail?.message);
      setNonce(n => n + 1);
      setActive(true);
    };
    window.addEventListener("license-activated-celebration", handler);
    return () => window.removeEventListener("license-activated-celebration", handler);
  }, []);

  return (
    <AnimatePresence>
      {active && (
        <LicenseConfetti
          key={nonce}
          message={message}
          onDone={() => setActive(false)}
        />
      )}
    </AnimatePresence>
  );
}

/** Helper used by activation handlers — dispatches the celebration event.
 *  Also fires `commander-dismiss-dialogs` so any open modals collapse
 *  before the confetti burst covers the screen. */
export function fireLicenseCelebration(opts?: { message?: string; toast?: string }) {
  window.dispatchEvent(new CustomEvent("commander-dismiss-dialogs"));
  window.dispatchEvent(new CustomEvent("license-activated-celebration", { detail: opts }));
}
