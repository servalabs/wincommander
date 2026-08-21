import { Icon } from "@/components/ui/bp";
import { useMemo, useCallback } from "react";
import { getSidebarManifests, type PanelId } from "../../types/panels";
import { ALL_TOGGLES } from "../../registry";
import { getToggleVisibility, resolveToggleText } from "../../types/toggles";
import useVisibility from "../../hooks/useVisibility";

interface GlobalSearchNoResultsProps {
  searchQuery: string;
  currentPanelId: PanelId;
}

export default function GlobalSearchNoResults({ searchQuery, currentPanelId }: GlobalSearchNoResultsProps) {
  const visibility = useVisibility();
  const navigateToPanel = useCallback((id: PanelId) => {
    window.dispatchEvent(new CustomEvent('navigate-panel', { detail: id }));
  }, []);

  const globalMatches = useMemo(() => {
    if (!searchQuery.trim()) return [];
    const q = searchQuery.toLowerCase().trim();
    const currentLevel = visibility.density === "expert" ? "advanced" : "standard";
    const visibleManifests = getSidebarManifests(visibility);

    const matches: Array<{ id: PanelId; label: string; icon: any; type: 'panel' | 'setting' }> = [];

    // 1. Check panels
    visibleManifests.forEach(m => {
      if (m.id === currentPanelId) return;
      if (m.label.toLowerCase().includes(q) || m.searchKeywords?.some(k => k.toLowerCase().includes(q))) {
        matches.push({ id: m.id, label: m.label, icon: m.icon, type: 'panel' });
      }
    });

    // 2. Check toggles
    ALL_TOGGLES.forEach(t => {
      if (!visibility.isVisible(getToggleVisibility(t, visibility.profiles))) return;
      // Map domain to PanelId
      let panelId: PanelId = 'dashboard';
      if (t.domain === 'privacy') panelId = 'privacy';
      else if (t.domain === 'tweaks') panelId = 'tweaks';
      else if (t.domain === 'network') panelId = 'network';
      else if (t.domain === 'identity') panelId = 'system-identity';

      if (panelId === currentPanelId) return;

      const wording = resolveToggleText(t, currentLevel);
      const found = wording.label.toLowerCase().includes(q) || 
                    wording.description.toLowerCase().includes(q) ||
                    t.keywords?.some(k => k.toLowerCase().includes(q));
      
      if (found && !matches.some(m => m.id === panelId)) {
        const manifest = visibleManifests.find(m => m.id === panelId);
        if (manifest) {
          matches.push({ id: panelId, label: manifest.label, icon: manifest.icon, type: 'setting' });
        }
      }
    });

    return matches;
  }, [
    currentPanelId,
    searchQuery,
    visibility,
  ]);

  if (globalMatches.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-20 text-center animate-in fade-in zoom-in duration-300">
        <div className="w-16 h-16 rounded-full bg-[var(--color-bg-tertiary)] flex items-center justify-center mb-4 border border-[var(--color-border)]">
          <Icon icon="search" size={24} className="opacity-20" />
        </div>
        <h3 className="text-lg font-bold text-[var(--color-text-primary)] mb-2">No results found in this panel</h3>
        <p className="text-sm text-[var(--color-text-muted)] max-w-md">
          We couldn't find any settings matching "{searchQuery}" here. Try checking other sections or use the global search.
        </p>
      </div>
    );
  }

  return (
    <div className="flex flex-col items-center justify-center py-12 animate-in fade-in slide-in-from-bottom-4 duration-500">
      <div className="w-12 h-12 rounded-full bg-[var(--color-accent-dim)] flex items-center justify-center mb-4 border border-[var(--color-accent)]/30">
        <Icon icon="globe" size={20} className="text-[var(--color-accent)]" />
      </div>
      <h3 className="text-lg font-bold text-[var(--color-text-primary)] mb-2">Found in other panels</h3>
      <p className="text-sm text-[var(--color-text-muted)] mb-8">
        Your search "{searchQuery}" matches items in the following sections:
      </p>

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 w-full max-w-4xl">
        {globalMatches.map(m => (
          <button
            key={m.id}
            onClick={() => navigateToPanel(m.id)}
            className="flex items-center gap-4 p-4 rounded-xl bg-[var(--color-bg-secondary)] border border-[var(--color-border)] hover:border-[var(--color-accent)] hover:bg-[var(--color-bg-tertiary)] transition-all group text-left"
          >
            <div className="w-10 h-10 rounded-lg bg-[var(--color-bg-tertiary)] group-hover:bg-[var(--color-accent-dim)] flex items-center justify-center border border-[var(--color-border)] group-hover:border-[var(--color-accent)]/30 transition-colors">
              <Icon icon={m.icon} size={16} className="group-hover:text-[var(--color-accent)]" />
            </div>
            <div className="flex-1">
              <div className="text-xs font-mono uppercase opacity-40 mb-0.5 tracking-tighter">Matches {m.type}</div>
              <div className="font-bold text-[var(--color-text-primary)] group-hover:text-[var(--color-accent)] transition-colors">{m.label}</div>
            </div>
            <Icon icon="chevron-right" size={14} className="opacity-0 group-hover:opacity-100 -translate-x-2 group-hover:translate-x-0 transition-all" />
          </button>
        ))}
      </div>
    </div>
  );
}
