import { Icon, type IconName } from "../../../components/ui/icon";

export const MAINTENANCE_WORKSPACES: Array<{ id: string; label: string; icon: IconName; description: string }> = [
  { id: "cleanup", label: "Deep cleanup", icon: "clean", description: "Review regenerable caches and Windows storage." },
  { id: "large", label: "Large files", icon: "document", description: "Find the files taking up the most space." },
  { id: "duplicates", label: "Duplicate cleanup", icon: "duplicate", description: "Compare identical files and keep a verified copy." },
  { id: "analysis", label: "Disk space analysis", icon: "hard-drive", description: "Explore folders and understand disk usage." },
  { id: "startup", label: "Startup & drivers", icon: "power", description: "Review startup programs and installed drivers." },
  { id: "registry", label: "Registry & hygiene", icon: "settings", description: "Review registry entries, shortcuts, and application leftovers." },
  { id: "history", label: "Operation history", icon: "history", description: "Review the outcomes of maintenance operations." },
];

export function MaintenanceWorkspaceNav({ active, onChange }: { active: string; onChange: (id: string) => void }) {
  return <nav className="maintenance-workspace-nav" aria-label="Maintenance tools">
    <span className="maintenance-nav-label">Storage & system</span>
    {MAINTENANCE_WORKSPACES.map((item) => <button key={item.id} type="button" className="maintenance-workspace-nav-item" aria-current={active === item.id ? "page" : undefined} onClick={() => onChange(item.id)}>
      <Icon icon={item.icon} size={17} /><span>{item.label}</span>
    </button>)}
    <p className="maintenance-nav-note">Scan first. Review every selection before making changes.</p>
  </nav>;
}
