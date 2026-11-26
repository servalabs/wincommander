import ChangePasswordDialog from "./ChangePasswordDialog";
import RenameVaultDialog from "./RenameVaultDialog";
import type { VaultFileRequest } from "@/lib/vaultFileActions";
// The dialogs also open from the sidebar and the Fleet panel, before Secure Storage has loaded its styles.
import "./index.css";

export default function VaultFileDialogs({ request, onClose }: { request: VaultFileRequest; onClose: () => void }) {
  return request.dialog === "password"
    ? <ChangePasswordDialog key={request.id} isOpen={request.open} onClose={onClose} initialPath={request.path} />
    : <RenameVaultDialog key={request.id} isOpen={request.open} onClose={onClose} initialPath={request.path} />;
}
