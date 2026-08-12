import { Icon, type IconName } from "@/components/ui/bp";
import type { ReactNode } from "react";
import type { Density } from "../../types/persona";
import "./AdaptiveCard.css";

interface AdaptiveCardProps {
  density: Density;
  title: string;
  description?: string;
  icon?: IconName;
  meta?: ReactNode;
  actions?: ReactNode;
  className?: string;
  children?: ReactNode;
}

export default function AdaptiveCard({
  density,
  title,
  description,
  icon,
  meta,
  actions,
  className = "",
  children,
}: AdaptiveCardProps) {
  const variant = density === "expert" ? "row" : "tile";

  return (
    <article className={`adaptive-card adaptive-card--${variant} ${className}`.trim()}>
      {icon ? (
        <div className="adaptive-card__icon" aria-hidden="true">
          <Icon icon={icon} size={density === "expert" ? 14 : 18} />
        </div>
      ) : null}
      <div className="adaptive-card__body">
        <div className="adaptive-card__head">
          <div>
            <h3>{title}</h3>
            {description ? <p>{description}</p> : null}
          </div>
          {meta ? <div className="adaptive-card__meta">{meta}</div> : null}
        </div>
        {children ? <div className="adaptive-card__content">{children}</div> : null}
      </div>
      {actions ? <div className="adaptive-card__actions">{actions}</div> : null}
    </article>
  );
}
