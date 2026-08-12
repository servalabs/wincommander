// Info affordances for the network maintenance cards, built on the shadcn/v2
// kit so NetworkMaintenanceTools stays single-kit.
//
// Two shapes, deliberately: InfoTip for a one-line clarification next to a
// control or a column header, InfoPopover for a card-level explanation that
// needs paragraphs. Per PanelHeader.tsx, per-panel "?" chrome was removed in
// favour of the title-bar guide — these are section/column level only.
import type { ReactNode } from "react";
import { Icon } from "../../components/ui/icon";
import { Popover, PopoverContent, PopoverTrigger } from "../../components/ui/popover";
import { Tooltip, TooltipContent, TooltipTrigger } from "../../components/ui/tooltip";

export function InfoTip({ content, label }: { content: ReactNode; label: string }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          aria-label={label}
          // Rows in the firewall table toggle selection on click; an info button
          // inside one must not double as a row click.
          onClick={(e) => e.stopPropagation()}
          className="inline-grid size-4 shrink-0 place-items-center rounded-full text-[var(--text-mute)] transition-colors hover:text-[var(--accent)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-line)]"
        >
          <Icon icon="info-sign" size={11} />
        </button>
      </TooltipTrigger>
      <TooltipContent className="max-w-[280px] leading-relaxed">{content}</TooltipContent>
    </Tooltip>
  );
}

export function InfoPopover({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="inline-flex items-center gap-1 rounded-[var(--r-sm)] border border-[var(--accent-line)] px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--accent)] transition-colors hover:bg-[var(--accent-soft)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-line)]"
        >
          <Icon icon="info-sign" size={11} />
          How it works
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[340px]">
        <div className="mb-2 font-[family-name:var(--font-display)] text-[13px] font-semibold text-[var(--text)]">
          {title}
        </div>
        <div className="flex flex-col gap-2 text-[12px] leading-relaxed text-[var(--text-dim)]">
          {children}
        </div>
      </PopoverContent>
    </Popover>
  );
}
