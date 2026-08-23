import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const panel = readFileSync("src/panels/fleet/index.tsx", "utf8");
const access = readFileSync("src/panels/fleet/AccessControlTab.tsx", "utf8");
const vault = readFileSync("src/panels/fleet/VaultAccessTab.tsx", "utf8");
const vaultPresentation = readFileSync("src/panels/fleet/vaultAccessPresentation.ts", "utf8");
const patternPicker = readFileSync("src/panels/fleet/VaultAccessPatternPicker.tsx", "utf8");
const vaultHook = readFileSync("src/hooks/useVaultAccess.ts", "utf8");
const vaultDraft = readFileSync("src/panels/fleet/vaultAccessDraft.ts", "utf8");
const volumeEditor = readFileSync("src/panels/fleet/VaultVolumesEditor.tsx", "utf8");
const infoPopover = readFileSync("src/panels/fleet/FleetInfoPopover.tsx", "utf8");
const css = readFileSync("src/panels/fleet/index.css", "utf8");
const cleanupBackend = readFileSync("src-tauri/commander-free/scripts/modules/privacy/cleanup.ps1", "utf8");

describe("Fleet access-control panel contracts", () => {
  test("keeps only enrollment, universal access control, and Vault permissions", () => {
    expect(panel).toContain("Enrollment");
    expect(panel).toContain("Access control");
    expect(panel).toContain("Vault permissions");
    expect(panel).not.toContain("FleetConsoleLink");
    expect(panel).not.toContain("ClipboardGuardTab");
    expect(panel).not.toContain("InkReceiptTab");
  });

  test("keeps non-admin users on My vaults without administrative Fleet tabs", () => {
    expect(panel).toContain("getCapabilities()");
    expect(panel).toContain("capabilities.can_manage_policy");
    expect(panel).not.toContain("systemInfo?.isAdmin");
    expect(panel).toContain('value={activeTab} onValueChange={setActiveTab}');
    expect(panel).toContain('{isAdmin && <TabsTrigger value="enrollment">Enrollment</TabsTrigger>}');
    expect(panel).toContain('{isAdmin && <TabsTrigger value="access-control">Access control</TabsTrigger>}');
    expect(panel).toContain('{isAdmin ? "Vault permissions" : "My vaults"}');
    expect(panel).toContain('<VaultAccessTab isAdmin={isAdmin} directory={directory} />');
  });

  test("retries transient service failures instead of permanently demoting an administrator", () => {
    expect(panel).toContain("retriesRemaining > 0");
    expect(panel).toContain("setTimeout(() => { void probe(retriesRemaining - 1); }, 750)");
    expect(panel).toContain('window.addEventListener("focus", refreshOnFocus)');
    expect(panel).toContain('setActiveTab("vault")');
    expect(panel).toContain('setCapabilityState("unavailable")');
    expect(panel).toContain("Retry permission check");
    expect(panel).toContain('if (canManage && lastCapability.current !== true) setActiveTab("enrollment")');
  });

  test("uses one checkbox directory and sorts selected users first", () => {
    expect(access).toContain('type="checkbox"');
    expect(access).toContain("membershipOrder");
    expect(access).toContain("createAccessGroup");
  });

  test("discovers new Windows accounts before their first sign-in", () => {
    expect(access).toContain("getFleetAccessUsers");
    expect(access).not.toContain("getUserProfiles");
    const fleetDiscovery = cleanupBackend.slice(
      cleanupBackend.indexOf("function Get-FleetAccessUsers"),
      cleanupBackend.indexOf("function Get-LoggedInUsers"),
    );
    expect(fleetDiscovery).toContain("Win32_UserAccount");
    expect(fleetDiscovery).not.toContain("Win32_UserProfile");
  });

  test("keeps a 30/70 layout without nested Access Control scrolling", () => {
    expect(css).toContain("grid-template-columns: minmax(250px, 3fr) minmax(0, 7fr)");
    expect(access).not.toContain("ScrollArea");
    expect(access).not.toContain("Add Windows account name");
    expect(access).not.toContain("addManualUser");
  });

  test("uses the mockup-aligned Vault permissions workspace with per-group permissions", () => {
    expect(volumeEditor).toContain("fleet-vault-workspace");
    expect(volumeEditor).toContain("fleet-vault-matrix");
    expect(volumeEditor).toContain("Owner / administrator");
    expect(volumeEditor).toContain("No access");
    expect(volumeEditor).toContain("Read & write");
    expect(css).toContain("fleet-vault-empty-inline");
    expect(css).toContain("overflow-x: auto");
  });

  test("keeps Vault owner and drive-letter fields readable at narrow widths", () => {
    expect(vault).toContain('className="fleet-owner-inputs"');
    expect(vault).toContain("Drive letter");
    expect(css).toContain("container-type: inline-size;");
    expect(css).toContain("grid-template-columns: minmax(160px, 1fr) minmax(220px, 1.6fr) minmax(220px, 1.4fr) minmax(92px, 0.55fr);");
    expect(css).toContain("@container (max-width: 960px)");
    expect(css).toContain("@container (max-width: 600px)");
    expect(css).toContain(".fleet-owner-inputs { grid-template-columns: repeat(2, minmax(0, 1fr)); }");
    expect(css).toContain(".fleet-owner-inputs { grid-template-columns: 1fr; }");
  });

  test("keeps the everyday Vault workflow primary and technical recovery details secondary", () => {
    expect(vault).toContain("1. Vault details");
    expect(vault).toContain("3. Add Windows users or groups");
    expect(vault).toContain("Advanced and recovery");
    expect(patternPicker).toContain("2. Choose who can use this vault");
    expect(css).toContain(".fleet-vault-advanced");
  });

  test("explains every Vault setting and hides raw service bookkeeping", () => {
    expect(vault).toContain("The label people recognize.");
    expect(vault).toContain("The encrypted .hc file on this PC.");
    expect(vault).toContain("The Windows account responsible for this Vault.");
    expect(vault).toContain("The preferred letter in File Explorer.");
    expect(vault).toContain("gives that account or team the access selected above");
    expect(vaultPresentation).toContain("Saved to Windows");
    expect(vaultPresentation).toContain("Mounting is unavailable until this is fixed");
    expect(vault).not.toContain("Service verification details");
    expect(vault).not.toContain("does not establish live mount acceptance");
    expect(vault).not.toContain("Policy: {status?.policy_id");
    expect(vault).not.toContain("Validation: {status?.validation_state");
  });

  test("offers one compact manual refresh action", () => {
    expect(vault.match(/onClick=\{\(\) => void refresh\(\)\}/g)).toHaveLength(1);
    expect(vault).toContain('<Icon icon="refresh" size={14} />');
    expect(vault).toContain(">Refresh</Button>");
    expect(vault).not.toContain("Refresh Vault access");
    expect(vault).not.toContain("Refresh status");
  });

  test("keeps the user and group editor separate from the permissions table layout", () => {
    expect(vault).toContain('className="fleet-vault-grants"');
    expect(vault).toContain('className="fleet-vault-grant-row"');
    expect(vault).not.toContain('<div className="fleet-vault-matrix">');
    expect(css).toContain(".fleet-vault-grants");
    expect(css).toContain(".fleet-vault-grant-row");
  });

  test("opens real information popovers for access groups and Vault permissions", () => {
    expect(infoPopover).toContain("PopoverTrigger");
    expect(infoPopover).toContain("PopoverContent");
    expect(access).toContain("How access groups work");
    expect(volumeEditor).toContain("How Vault access is decided");
  });

  test("uses the service-owned Vault policy and makes old planner data opt-in only", () => {
    expect(vault).toContain("useVaultAccess<VaultAccessPolicy, VaultPolicyStatus>()");
    expect(vault).not.toContain("invoke(");
    expect(vaultHook).toContain('invoke<Policy>("get_vault_access_policy")');
    expect(vaultHook).toContain('invoke<Status>("apply_vault_access_policy"');
    expect(vault).toContain("version: policy.version + 1");
    expect(vault).toContain("Saved vaults");
    expect(vault).toContain("Future mounts only need the password");
    expect(vault).toContain("Save vault settings");
    expect(vaultPresentation).toContain("needs to refresh this older Vault status");
    expect(vaultPresentation).toContain("pending_mount_broker");
    expect(vault).toContain("own dedicated parent folder");
    expect(vault).toContain("Draft auto-saved on this PC");
    expect(vault).toContain("dirtyRef.current");
    expect(vault).toContain("Rebase draft with saved settings");
    expect(vaultDraft).toContain("rebaseVaultAccessDraft");
    expect(vault).toContain("if (replaceDirtyDraft || !dirtyRef.current)");
    expect(vault).not.toContain("setLoading(true)");
    expect(vaultDraft).toContain("window.localStorage");
    expect(vaultDraft).toContain("never count as applied policy");
    expect(vaultDraft).not.toContain("password");
    expect(panel).not.toContain("loadVaultPolicy");
    expect(panel).not.toContain("saveVaultPolicy");
  });

  test("gives non-admin users a caller-filtered Vault mount surface", () => {
    expect(vaultHook).toContain('invoke<VaultAuthorizedEntry[]>("vault_list_authorized_entries")');
    expect(vaultHook).toContain('invoke<VaultMountEntryResult>("vault_mount_entry", { entryId, password })');
    expect(vaultHook).toContain('invoke<VaultMountEntryResult>("vault_unmount_entry", { entryId })');
    expect(vaultHook).not.toContain("volumePath");
    expect(vault).toContain("My vaults");
    expect(vault).toContain("Only Vaults that the service has authorized");
    expect(vault).toContain("{isAdmin && <>");
    expect(vault).toContain("passwordInputRef");
    expect(vault).toContain('if (input) input.value = ""');
    expect(vault).toContain('password = ""');
    expect(vault).not.toContain("setMountPassword");
    expect(vault).toContain("vaultPresentationLabel(entry.presentation)");
  });
});
