import { Icon } from "@/components/ui/bp";

/**
 * Free cannot attest VeraCrypt pre-boot encryption from install artefacts.
 * Keep the panel explicit instead of launching a PowerShell command that can
 * only return an unsupported/unknown result.
 */
function SystemEncryptionSection() {
  return (
    <div className="vault-sys-enc-row is-unknown" role="status">
      <Icon icon="help" size={12} />
      <span className="vault-sys-enc-label">System drive</span>
      <span>·</span>
      <span>Verification unavailable</span>
    </div>
  );
}

export default SystemEncryptionSection;
