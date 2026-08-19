import type { ReactNode } from "react";
import { buttonVariants } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";

export default function FleetInfoPopover({
  label,
  title,
  description,
  children,
}: {
  label: string;
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <Popover>
      <PopoverTrigger
        aria-label={label}
        className={buttonVariants({ size: "icon", variant: "ghost", className: "fleet-info-trigger" })}
        title={label}
      >
        <Icon icon="info-sign" />
      </PopoverTrigger>
      <PopoverContent align="end" className="fleet-info-popover">
        <div className="fleet-info-heading">
          <Icon icon="info-sign" />
          <strong>{title}</strong>
        </div>
        <p>{description}</p>
        {children}
      </PopoverContent>
    </Popover>
  );
}
