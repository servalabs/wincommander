import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import VaultActionMenu, { type VaultMenuItem } from "@/components/shared/VaultActionMenu";

export default function VaultPolicyMoreMenu({ label, items }: { label: string; items: VaultMenuItem[] }) {
  return (
    <VaultActionMenu items={items} trigger={
      <Button variant="outline" size="sm" className="px-2" aria-label={`More actions for ${label}`} title="More actions">
        <Icon icon="more" size={14} />
      </Button>
    } />
  );
}
