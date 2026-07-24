import { useEffect, useMemo, useState } from "react";
import { Icon } from "@/components/ui/bp";
import { brandIconCandidates, fallbackIcon } from "./appIcons";

interface AppIconProps {
  id: string;
  category: string;
  iconData?: string | null;
  size?: number;
}

// Tries each bundled brand asset in order; if all fail to load, renders a
// Blueprint icon picked from the app's category.
export default function AppIcon({ id, category, iconData, size = 28 }: AppIconProps) {
  const [attempt, setAttempt] = useState(0);
  const candidates = useMemo(() => brandIconCandidates(id), [id]);

  useEffect(() => {
    setAttempt(0);
  }, [id]);

  // Curated brand SVG/PNG beats extracted EXE icon — Apple bundles and
  // similar components embed generic Windows icons in their EXEs, so we
  // always try the static asset first. iconData is the fallback for apps
  // with no curated file (TeamViewer, ExifTool, etc.).
  //
  // loading="eager" is intentional: lazy defers the request until the img
  // enters the viewport, so onError never fires for off-screen cards and
  // the chain to iconData never completes. Icons are 28px — eager is fine.
  if (attempt < candidates.length) {
    return (
      <img
        key={`${id}-${attempt}`}
        src={candidates[attempt]}
        alt=""
        className="app-icon-img"
        width={size}
        height={size}
        onError={() => setAttempt((current) => Math.min(current + 1, candidates.length))}
        loading="eager"
      />
    );
  }

  if (iconData) {
    return (
      <img
        key={`${id}-data`}
        src={iconData}
        alt=""
        className="app-icon-img"
        width={size}
        height={size}
        loading="eager"
      />
    );
  }

  return (
    <div className="app-icon-fallback" style={{ width: size, height: size }}>
      <Icon icon={fallbackIcon(category)} size={Math.round(size * 0.55)} />
    </div>
  );
}
