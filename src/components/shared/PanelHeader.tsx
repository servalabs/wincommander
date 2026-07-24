// src/components/shared/PanelHeader.tsx
//
// Standard panel title block — a big heading + a plain-English description.
// Every panel except the Dashboard renders one at the top so each section
// opens with a clear "what is this / what can I do here" (owner request).
//
// The per-panel help "?" was removed in favour of the single title-bar
// "Help & guide" entry (they opened the same help center — a duplicate surface).
// The per-panel "Take the tour" pill was removed for the same reason: Help &
// Guide's "Replay tour" already starts the current panel's tour.

import type { PanelId } from "../../types/panels";

interface PanelHeaderProps {
  title: string;
  description: string;
  /** Retained for call-site compatibility; no longer drives a per-panel help "?". */
  panelId?: PanelId;
}

export default function PanelHeader({ title, description }: PanelHeaderProps) {
  return (
    <div className="panel-header">
      <div className="panel-header-row">
        <h1 className="panel-header-title">{title}</h1>
      </div>
      <p className="panel-header-desc">{description}</p>
    </div>
  );
}
