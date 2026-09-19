import type { ComponentProps } from "react";
import DecoyMonitorSection from "./DecoyMonitorSection";
import CanaryTokensSection from "./CanaryTokensSection";

type DecoyMonitorProps = ComponentProps<typeof DecoyMonitorSection>;

export interface DeceptionTripwiresSectionProps {
  decoy: DecoyMonitorProps;
}

export default function DeceptionTripwiresSection({
  decoy,
}: DeceptionTripwiresSectionProps) {
  return (
    <section
      aria-labelledby="deception-tripwires-title"
      className="flex flex-col gap-3"
    >
      <header className="rounded border border-[var(--shield-inner-border)] bg-[var(--surface-2)] px-4 py-3">
        <h2
          id="deception-tripwires-title"
          className="text-sm font-semibold text-[var(--shield-text)]"
        >
          Deception &amp; Tripwires
        </h2>
        <p className="mt-1 text-xs text-[var(--shield-text-subtle)]">
          Two separate tripwires: local decoy-file activity and local-only canary beacon requests.
        </p>
      </header>
      <div
        className="grid gap-3"
        role="group"
        aria-label="Deception and tripwire controls"
      >
        <DecoyMonitorSection {...decoy} />
        <CanaryTokensSection />
      </div>
    </section>
  );
}
