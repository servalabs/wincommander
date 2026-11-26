import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import VaultActionMenu from "@/components/shared/VaultActionMenu";
import type { VaultFileActions } from "@/lib/vaultFileActions";

/** Entry point for a container no list shows, such as a file WinCommander has no record of. */
export function OtherVaultFileMenu({ actions }: { actions: VaultFileActions }) {
  return (
    <div className="vault-file-other">
      <VaultActionMenu
        trigger={<Button variant="ghost" size="sm"><Icon icon="document" size={14} />Other vault file…</Button>}
        items={[
          { label: "Change Password…", onSelect: () => actions.changePassword({ path: null }) },
          { label: "Rename…", onSelect: () => actions.rename({ path: null }) },
        ]} />
    </div>
  );
}
