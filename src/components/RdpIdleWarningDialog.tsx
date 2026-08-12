/**
 * RdpIdleWarningDialog
 *
 * Full-screen overlay shown when the user is about to be auto-disconnected
 * from their RDP session due to inactivity. Displays a countdown and lets
 * the user dismiss it by clicking "I'm still here".
 */
import { AnimatePresence, motion } from "framer-motion";
interface Props {
  isOpen: boolean;
  secondsLeft: number;
  idleSeconds: number;
  timeoutSeconds: number;
  warningWindowSeconds: number;
  title?: string;
  message?: string;
  buttonLabel?: string;
  footnote?: string;
  onDismiss: () => void;
}

export default function RdpIdleWarningDialog({
  isOpen,
  secondsLeft,
  idleSeconds,
  warningWindowSeconds,
  title = "Are you still there?",
  message,
  buttonLabel = "I'm still here",
  footnote = "Move your mouse to cancel automatically",
  onDismiss,
}: Props) {
  const pct = Math.max(0, Math.min(1, secondsLeft / Math.max(1, warningWindowSeconds)));
  const mins = Math.floor(idleSeconds / 60);
  const secs = idleSeconds % 60;
  const idleLabel = mins > 0 ? `${mins}m ${secs}s` : `${secs}s`;

  // Colour shifts red as countdown approaches 0
  const urgency = 1 - pct; // 0 = calm, 1 = urgent
  const ringColor = urgency > 0.6
    ? "#ef4444"   // red
    : urgency > 0.3
      ? "#f97316" // orange
      : "#f59e0b"; // amber

  const circumference = 2 * Math.PI * 40;
  const strokeDash = circumference * pct;

  return (
    <AnimatePresence>
      {isOpen && (
        <motion.div
          key="rdp-idle-overlay"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.2 }}
          className="fixed inset-0 z-[9999] flex items-center justify-center"
          style={{ background: "rgba(0,0,0,0.72)", backdropFilter: "blur(6px)" }}
        >
          <motion.div
            initial={{ scale: 0.92, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            exit={{ scale: 0.92, opacity: 0 }}
            transition={{ duration: 0.2 }}
            className="flex flex-col items-center gap-6 rounded-2xl p-10 text-center"
            style={{
              background: "var(--color-surface, #1a1a2e)",
              border: "1px solid rgba(255,255,255,0.08)",
              boxShadow: "0 24px 80px rgba(0,0,0,0.6)",
              minWidth: 340,
            }}
          >
            {/* Countdown ring */}
            <div className="relative flex items-center justify-center" style={{ width: 100, height: 100 }}>
              <svg width="100" height="100" style={{ position: "absolute", transform: "rotate(-90deg)" }}>
                {/* Track */}
                <circle
                  cx="50" cy="50" r="40"
                  fill="none"
                  stroke="rgba(255,255,255,0.08)"
                  strokeWidth="6"
                />
                {/* Progress */}
                <circle
                  cx="50" cy="50" r="40"
                  fill="none"
                  stroke={ringColor}
                  strokeWidth="6"
                  strokeLinecap="round"
                  strokeDasharray={`${strokeDash} ${circumference}`}
                  style={{ transition: "stroke-dasharray 0.9s linear, stroke 0.5s ease" }}
                />
              </svg>
              <span
                className="text-3xl font-bold tabular-nums"
                style={{ color: ringColor, transition: "color 0.5s ease", zIndex: 1 }}
              >
                {secondsLeft}
              </span>
            </div>

            {/* Title */}
            <div className="flex flex-col gap-2">
              <span className="text-lg font-semibold" style={{ color: "var(--color-text-primary, #f0f0f0)" }}>
                {title}
              </span>
              <span className="text-sm" style={{ color: "var(--color-text-muted, #888)", maxWidth: 260 }}>
                {message ?? (
                  <>
                    You've been inactive for <strong style={{ color: "var(--color-text-primary, #f0f0f0)" }}>{idleLabel}</strong>.
                    Your RDP session will close automatically in {secondsLeft}s.
                  </>
                )}
              </span>
            </div>

            {/* Action */}
            <button
              onClick={onDismiss}
              className="px-6 py-2.5 rounded-xl text-sm font-semibold transition-all"
              style={{
                background: ringColor,
                color: "#fff",
                border: "none",
                cursor: "pointer",
                boxShadow: `0 0 20px ${ringColor}55`,
              }}
            >
              {buttonLabel}
            </button>

            <span className="text-[11px]" style={{ color: "var(--color-text-muted, #666)" }}>
              {footnote}
            </span>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
