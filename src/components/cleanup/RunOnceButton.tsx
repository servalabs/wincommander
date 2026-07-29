import { Icon } from "@/components/ui/bp";

interface RunOnceButtonProps {
  isRunning: boolean;
  disabled?: boolean;
  onClick: () => void;
  className?: string;
}

/** Shared one-shot action control across System Cleanup. */
export default function RunOnceButton({
  isRunning,
  disabled = false,
  onClick,
  className = "",
}: RunOnceButtonProps) {
  return (
    <button
      type="button"
      disabled={isRunning || disabled}
      onClick={(event) => {
        event.stopPropagation();
        onClick();
      }}
      className={`cleanup-run-once-button ${className}`}
    >
      <Icon icon="play" size={10} color="currentColor" />
      {isRunning ? "Running…" : "Run once"}
    </button>
  );
}
