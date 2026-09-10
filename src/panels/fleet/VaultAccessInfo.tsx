import type { ReactNode } from "react";
import { Icon } from "@/components/ui/icon";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

export default function VaultAccessInfo({ label, children }: { label: string; children: ReactNode }) {
  return <Tooltip>
    <TooltipTrigger type="button" aria-label={label} className="vault-access-info-trigger">
      <Icon icon="info-sign" size={14} aria-hidden="true" />
    </TooltipTrigger>
    <TooltipContent className="vault-access-info-content" side="top" collisionPadding={12}>
      {children}
    </TooltipContent>
  </Tooltip>;
}
