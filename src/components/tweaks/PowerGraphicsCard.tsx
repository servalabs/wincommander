import PowerPlanCard from "./PowerPlanCard";
import SectionCard from "../shared/SectionCard";
import ToggleSection from "../shared/ToggleSection";
import type { ToggleDef } from "../../types/toggles";
import type { SectionDef } from "../../types/toggles";

interface PowerGraphicsCardProps {
  powerSection: SectionDef;
  gpuSection?: SectionDef;
  toggles: ToggleDef[];
  onToggled: (toggle: ToggleDef) => Promise<void>;
  searchQuery: string;
}

/**
 * Keeps the decisions that affect heat, battery, and graphics in one compact
 * card. The plan is deliberately the wider first row; the two smaller groups
 * share the second row instead of stranding either beside blank space.
 */
export default function PowerGraphicsCard({
  powerSection,
  gpuSection,
  toggles,
  onToggled,
  searchQuery,
}: PowerGraphicsCardProps) {
  return (
    <SectionCard title="Power & Graphics" icon="flash">
      <div className={`power-graphics-card ${gpuSection ? "" : "power-graphics-card--single"}`}>
        <div className="power-graphics-card__plan">
          <PowerPlanCard bare />
        </div>
        <div className="power-graphics-card__group">
          <p className="power-graphics-card__label">Power management</p>
          <ToggleSection
            section={powerSection}
            toggles={toggles}
            onToggled={onToggled}
            searchQuery={searchQuery}
            bare
            gridClassName="!grid-cols-1 !gap-2"
          />
        </div>
        {gpuSection && (
          <div className="power-graphics-card__group">
            <p className="power-graphics-card__label">Graphics — detected vendor</p>
            <ToggleSection
              section={gpuSection}
              toggles={toggles}
              onToggled={onToggled}
              searchQuery={searchQuery}
              bare
              gridClassName="!grid-cols-1 !gap-2"
            />
          </div>
        )}
      </div>
    </SectionCard>
  );
}
