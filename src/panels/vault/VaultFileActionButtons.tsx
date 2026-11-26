import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import VaultActionMenu from "@/components/shared/VaultActionMenu";
import type { VaultFileActions } from "@/lib/vaultFileActions";

/** The Encrypted Volumes More menu: each item asks for the vault file, so it reaches files no list shows. */
export function VaultFileMoreMenu({ actions }: { actions: VaultFileActions }) {
  return (
    <VaultActionMenu items={[
      { label: "Change Password…", onSelect: () => actions.changePassword({ path: null }) },
      { label: "Rename…", onSelect: () => actions.rename({ path: null }) },
    ]} trigger={
      <Button variant="outline" size="sm" className="px-2 vault-card-more" aria-label="More vault actions" title="More vault actions">
        <Icon icon="more" size={14} />
      </Button>
    } />
  );
}
