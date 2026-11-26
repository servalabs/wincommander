import { Fragment, type ReactNode } from "react";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";

export interface VaultMenuItem {
  label: string;
  onSelect: () => void;
  /** Shown as the item's tooltip; a reason disables the item. */
  disabledReason?: string | null;
  isDanger?: boolean;
  hasSeparatorBefore?: boolean;
}

/** `trigger` must forward its ref and props, like the app's `@/components/ui/button` Button. */
export default function VaultActionMenu({ trigger, items }: { trigger: ReactNode; items: VaultMenuItem[] }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{trigger}</DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        {items.map(item => (
          <Fragment key={item.label}>
            {item.hasSeparatorBefore && <DropdownMenuSeparator />}
            {/* A disabled item takes no pointer events, so its reason sits on a wrapper. */}
            <div title={item.disabledReason ?? undefined}>
              <DropdownMenuItem disabled={Boolean(item.disabledReason)} onSelect={item.onSelect}
                className={item.isDanger ? "text-[var(--danger)]" : undefined}>
                {item.label}
              </DropdownMenuItem>
            </div>
          </Fragment>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
