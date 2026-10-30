import type { DiskAnalyzerMode } from "@/lib/diskAnalyzer";

const TABS: { id: DiskAnalyzerMode; label: string }[] = [{ id: "space", label: "Space" }, { id: "large", label: "Large Files" }, { id: "types", label: "File Types" }];

export default function DiskAnalyzerTabs({ mode, disabled, onChange }: { mode: DiskAnalyzerMode; disabled: boolean; onChange: (mode: DiskAnalyzerMode) => void }) {
    function move(key: string) {
        const index = TABS.findIndex(tab => tab.id === mode);
        const next = key === "Home" ? 0 : key === "End" ? TABS.length - 1 : key === "ArrowRight" ? (index + 1) % TABS.length : key === "ArrowLeft" ? (index + TABS.length - 1) % TABS.length : -1;
        if (next < 0 || disabled) return false;
        onChange(TABS[next].id);
        requestAnimationFrame(() => document.getElementById(`disk-tab-${TABS[next].id}`)?.focus());
        return true;
    }
    return <div className="da-workspace-tabs" role="tablist" aria-label="Analyzer views">{TABS.map(tab => <button key={tab.id} type="button" role="tab" id={`disk-tab-${tab.id}`} aria-controls="disk-analyzer-view" aria-selected={mode === tab.id} tabIndex={mode === tab.id ? 0 : -1} disabled={disabled}
        onClick={() => onChange(tab.id)} onKeyDown={event => { if (move(event.key)) event.preventDefault(); }}>{tab.label}</button>)}</div>;
}
