import { Input } from "@/components/ui/input";
import type { VaultGroup } from "./vaultFleetTypes";
import FleetField from "./FleetField";

export default function VaultGroupsEditor({ groups, onChange }: {
  groups: VaultGroup[];
  onChange: (groups: VaultGroup[]) => void;
}) {
  const update = (id: VaultGroup["id"], patch: Partial<VaultGroup>) =>
    onChange(groups.map(group => group.id === id ? { ...group, ...patch } : group));

  return (
    <div className="fleet-group-grid">
      {groups.map(group => (
        <section className="fleet-subcard" key={group.id}>
          <h4>{group.id[0].toUpperCase() + group.id.slice(1)}</h4>
          <FleetField label="Windows group">
            <Input value={group.localGroup} onChange={event => update(group.id, { localGroup: event.target.value })} />
          </FleetField>
          <FleetField label="Standard users" hint="Comma-separated. Deployment removes privileged group memberships.">
            <Input
              value={group.users.join(", ")}
              onChange={event => update(group.id, {
                users: event.target.value.split(",").map(value => value.trim()).filter(Boolean),
              })}
            />
          </FleetField>
        </section>
      ))}
    </div>
  );
}
