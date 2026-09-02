import { getControlLifecycleTone, type ControlLifecycleState } from "@/lib/settingsControlLifecycle";
import StatusPill from "./StatusPill";

interface SettingsControlStatusProps {
  lifecycle: ControlLifecycleState;
}

/** Consistent status text for settings controls that have a lifecycle model. */
export default function SettingsControlStatus({ lifecycle }: SettingsControlStatusProps) {
  const account = lifecycle.account?.displayName ?? lifecycle.account?.name;
  const label = lifecycle.reason ? `${lifecycle.state}: ${lifecycle.reason}` : lifecycle.state;

  return (
    <span className="inline-flex items-center gap-2 flex-wrap">
      <StatusPill tone={getControlLifecycleTone(lifecycle.state)}>{label}</StatusPill>
      {account ? <span className="text-xs text-[var(--color-text-muted)]">Windows account: {account}</span> : null}
    </span>
  );
}
