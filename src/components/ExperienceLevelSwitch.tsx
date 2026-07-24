import { Icon } from "@/components/ui/bp";
import { useAppState } from "../context/AppContext";
import { useQueryClient } from "@tanstack/react-query";
import { settingsKeys } from "../hooks/queries/useSettingsQuery";
import type { Density } from "../types/persona";

export default function ExperienceLevelSwitch({ compact = false }: { compact?: boolean }) {
  const { patchAppSettings, appSettings } = useAppState();
  const queryClient = useQueryClient();
  const currentDensityValue: Density = appSettings?.app?.density ?? "guided";

  const handleDensityChange = async (newDensity: Density) => {
    try {
      // Clear persisted capabilities so they get recalculated from the new
      // density (otherwise old capabilities override).
      await patchAppSettings({
        app: {
          density: newDensity,
          capabilities: [],
        },
      });

      await queryClient.refetchQueries({ queryKey: settingsKeys.all });
      window.dispatchEvent(new CustomEvent('navigate-panel', { detail: 'dashboard' }));
    } catch (err) {
      console.error('Failed to change interface density:', err);
    }
  };

  const allDensities: { value: Density; label: string; icon: "home" | "cog" }[] = [
    { value: "guided", label: "Guided", icon: "home" },
    { value: "expert", label: "Expert", icon: "cog" },
  ];

  const currentDensity = allDensities.find((d) => d.value === currentDensityValue);

  const content = (
    <>
      <div className={`exp-header ${compact ? 'exp-header--compact' : ''}`}>
        <span className="exp-label">INTERFACE</span>
        <span className="exp-level-badge">
          {currentDensity?.label.toUpperCase()}
        </span>
      </div>
      <div className={`exp-track ${compact ? 'exp-track--compact' : ''}`}>
        {allDensities.map((density) => (
          <button
            key={density.value}
            className={`exp-btn ${compact ? 'exp-btn--compact' : ''} ${currentDensityValue === density.value ? 'active' : ''}`}
            onClick={() => handleDensityChange(density.value)}
            title={density.label}
          >
            <Icon className="exp-icon" icon={density.icon} size={compact ? 12 : 13} />
            <span className={`exp-btn-label ${compact ? 'exp-btn-label--compact' : ''}`}>{density.label}</span>
          </button>
        ))}
      </div>
    </>
  );

  return compact
    ? <div className="exp-col">{content}</div>
    : <div className="experience-switch">{content}</div>;
}
