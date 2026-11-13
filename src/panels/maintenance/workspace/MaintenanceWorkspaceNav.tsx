import { Icon, type IconName } from "../../../components/ui/icon";

export const MAINTENANCE_WORKSPACES: Array<{ id: string; label: string; icon: IconName; description?: string }> = [
  { id: "cleanup", label: "Deep cleanup", icon: "clean" },
  { id: "windows-storage", label: "Windows storage", icon: "hard-drive" },
  { id: "duplicates", label: "Duplicate cleanup", icon: "duplicate", description: "Compare identical files and keep a verified copy." },
  { id: "analysis", label: "Disk space analysis", icon: "hard-drive", description: "Explore folders and understand disk usage." },
  { id: "startup", label: "Startup & drivers", icon: "power", description: "Review startup programs and installed drivers." },
  { id: "registry", label: "Registry & hygiene", icon: "settings", description: "Review registry entries, shortcuts, and application leftovers." },
];

export function MaintenanceWorkspaceNav({ active, onChange }: { active: string; onChange: (id: string) => void }) {
  return <nav aria-label="Maintenance tools" className="maintenance-workspace-nav">
    <div className="inline-flex w-max min-w-full items-center justify-start gap-1 rounded-[var(--r)] bg-[var(--surface-2)] p-1">
      {MAINTENANCE_WORKSPACES.map((item) => <button key={item.id} type="button" onClick={() => onChange(item.id)} aria-current={active === item.id ? "page" : undefined} className="maintenance-workspace-nav-item" title={item.label}>
        <span className="inline-flex items-center gap-2"><Icon icon={item.icon} size={15} />{item.label}</span>
      </button>)}
    </div>
  </nav>;
}
