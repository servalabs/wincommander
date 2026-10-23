import { Icon, type IconName } from "../../../components/ui/icon";

export const MAINTENANCE_WORKSPACES: Array<{ id: string; label: string; icon: IconName; description: string }> = [
  { id: "cleanup", label: "Deep cleanup", icon: "clean", description: "Review browser, application and developer caches, with Windows storage tools in a separate section." },
  { id: "duplicates", label: "Duplicate cleanup", icon: "duplicate", description: "Compare identical files and keep a verified copy." },
  { id: "analysis", label: "Disk space analysis", icon: "hard-drive", description: "Explore folders and understand disk usage." },
  { id: "startup", label: "Startup & drivers", icon: "power", description: "Review startup programs and installed drivers." },
  { id: "registry", label: "Registry & hygiene", icon: "settings", description: "Review registry entries, shortcuts, and application leftovers." },
  { id: "history", label: "Operation history", icon: "history", description: "Review the outcomes of maintenance operations." },
];

export function MaintenanceWorkspaceNav({ active, onChange }: { active: string; onChange: (id: string) => void }) {
  return <nav className="maintenance-workspace-nav" aria-label="Maintenance tools">
    {MAINTENANCE_WORKSPACES.map((item) => <button key={item.id} type="button" className="maintenance-workspace-nav-item" title={item.label} aria-label={item.label} aria-current={active === item.id ? "page" : undefined} onClick={() => onChange(item.id)}>
      <Icon icon={item.icon} size={17} /><span>{item.label}</span>
    </button>)}
  </nav>;
}
