import { Icon, type IconName } from "../../../components/ui/icon";

export const MAINTENANCE_WORKSPACES: Array<{ id: string; label: string; icon: IconName; description: string }> = [
  { id: "cleanup", label: "Deep cleanup", icon: "clean", description: "Review storage used by browser and app caches, developer tools, and project artifacts." },
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
    {MAINTENANCE_WORKSPACES.map((item) => <button key={item.id} type="button" className="maintenance-workspace-nav-item" title={item.label} aria-label={item.label} aria-current={active === item.id ? "page" : undefined} onClick={() => onChange(item.id)}>
      <Icon icon={item.icon} size={17} /><span>{item.label}</span>
    </button>)}
    <button type="button" className="maintenance-workspace-nav-item" title="Privacy cleanup — opens System Cleanup" aria-label="Privacy cleanup — opens System Cleanup" onClick={() => window.dispatchEvent(new CustomEvent("navigate-panel", { detail: "cleanup" }))}>
      <Icon icon="shield" size={17} /><span>Privacy cleanup <small>System Cleanup</small></span>
    </button>
    <p className="maintenance-nav-note">Scan first. Review every selection before making changes.</p>
  </nav>;
}
