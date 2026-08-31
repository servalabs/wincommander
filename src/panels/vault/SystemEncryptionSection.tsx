import { Icon, Tooltip } from "@/components/ui/bp";

/**
 * Free cannot attest VeraCrypt pre-boot encryption from install artefacts.
 * Keep the panel explicit instead of launching a PowerShell command that can
 * only return an unsupported/unknown result.
 */
function SystemEncryptionSection() {
  return (
    <Tooltip content="System-drive encryption cannot be verified from this Secure Storage screen." position="top">
      <span className="vault-sys-enc-row is-unknown" role="status">
        <Icon icon="help" size={12} />
        <span className="vault-sys-enc-label">System encryption</span>
        <span className="vault-sys-enc-detail">Not verified</span>
      </span>
    </Tooltip>
  );
}

export default SystemEncryptionSection;
