import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import VaultAccessEditor from "./VaultAccessEditor";
import type { FleetAccessDirectory } from "./accessControlTypes";
import type { VaultAccessEntry, VaultAuthorizedEntry, VaultPolicyStatus } from "./vaultAccessTypes";
import { vaultMountResultLabel } from "./vaultAccessTypes";
import { vaultPolicyVerification } from "./vaultAccessPresentation";
import { vaultMountGate } from "./vaultAccessUiState";

// Synthetic identities only. No actual location, credential, or Vault is used.
const directory: FleetAccessDirectory = {
  schema: 1,
  users: [{ id: "example-user", username: "ExampleUser", displayName: "Example user" }],
  groups: [{ id: "example-group", name: "Example team", localGroup: "ExampleTeam", userIds: [] }],
};
const entry: VaultAccessEntry = {
  id: "example-vault", label: "Example vault", container_path: "", container_kind: "standard",
  owner_account: "ExampleUser", grants: [{ principal_name: "ExampleTeam", access: "read" }],
  mount: { presentation: "machine" },
};
const authorized: VaultAuthorizedEntry = {
  entry_id: entry.id, label: entry.label, access: "read", presentation: "machine",
  container_kind: "standard", mount_state: "unmounted", drive_letter: null,
};
const status: VaultPolicyStatus = {
  policy_id: "example-policy", version: 1, validation_state: "current", applied_at: 1,
  entries: [{ id: entry.id, result: "applied" }],
};
const root = readFileSync("src/panels/fleet/VaultAccessTab.tsx", "utf8");
const editorSource = readFileSync("src/panels/fleet/VaultAccessEditor.tsx", "utf8");
const helpSource = readFileSync("src/panels/fleet/VaultAccessInfo.tsx", "utf8");
const css = readFileSync("src/panels/fleet/VaultAccessEditor.css", "utf8");
const renderEditor = (value = entry) => renderToStaticMarkup(<VaultAccessEditor
  entry={value} entryIndex={0} directory={directory}
  onEntryChange={() => undefined} onOwnerChange={() => undefined} onPresetChange={() => undefined}
/>);

describe("Vault access editor presentation", () => {
  test("renders a native, initially closed Access details disclosure", () => {
    const html = renderEditor();
    const detailsTag = html.match(/<details\b[^>]*>/)?.[0];
    expect(detailsTag).toBeDefined();
    expect(detailsTag).not.toMatch(/\bopen(?:=|\s|>)/);
    expect(html).toContain("<summary>Access details</summary>");
    expect(html).toContain("Who can access this vault");
  });

  test("keeps general help out of the default render, with named keyboard controls", () => {
    const html = renderEditor();
    expect(html).toContain('aria-label="About primary owner"');
    expect(html).toContain('aria-label="About drive letter"');
    expect(html).not.toContain("The Windows account responsible for this Vault.");
    expect(html).not.toContain("The preferred letter in File Explorer.");
    expect(helpSource).toContain('TooltipTrigger type="button" aria-label={label}');
    expect(helpSource).toContain('from "@/components/ui/tooltip"');
    expect(helpSource).toContain("collisionPadding={12}");
    // Actual hover/focus/Escape interaction is covered by check-vault-access-ui.cjs.
  });

  test("keeps folder and grant-removal guidance outside collapsed help", () => {
    const visible = renderEditor().split('<details class="vault-access-details">')[0];
    expect(visible).toContain("own dedicated parent folder");
    expect(visible).toContain("Removing a grant takes effect only after saving.");
    expect(visible).toContain("Other user or group grants may still allow access.");
  });

  test("keeps the dual-container protection requirement visible only when relevant", () => {
    const warning = "A writable outer mount requires the hidden protection password for that one request.";
    expect(renderEditor()).not.toContain(warning);
    const visible = renderEditor({ ...entry, container_kind: "dual" }).split('<details class="vault-access-details">')[0];
    expect(visible).toContain(warning);
  });

  test("names each permission group, principal, access level, and removal action", () => {
    const html = renderEditor();
    expect(html).toContain('role="group" aria-label="Permission 1"');
    expect(html).toContain('aria-label="Grant 1 principal"');
    expect(html).toContain('aria-label="Grant 1 access"');
    expect(html).toContain('aria-label="Remove grant 1"');
    expect(html).toContain('value="ExampleTeam" selected=""');
    expect(html).toContain('aria-label="Grant 1 access">Read</output>');
  });

  test("preserves mixed read/write grants without introducing a deny value", () => {
    const html = renderEditor({ ...entry, grants: [
      { principal_name: "ExampleTeam", access: "read" },
      { principal_name: "ExampleUser", access: "write" },
    ] });
    expect(html).toContain("Custom access");
    expect(html).toContain('aria-label="Grant 2 access"');
    expect(html).toContain('value="write" selected="">Read &amp; write</option>');
    expect(html).not.toContain('<option value="none">');
    expect(html).not.toContain("This saved vault");
  });

  test("shows owner-only policy access without inventing a live authorization result", () => {
    const html = renderEditor({ ...entry, grants: [{ principal_name: entry.owner_account, access: "write" }], mount: { presentation: "per-user" } });
    expect(html).toContain('aria-label="Owner permission"');
    expect(html).toContain('aria-label="Owner policy access">Read &amp; write</output>');
    expect(html).not.toContain('aria-label="Remove grant 1"');
    expect(html).not.toContain("Access granted");
  });

  test("resets help on Edit/Manage access without replacing the focus or picker contract", () => {
    const navigation = root.slice(root.indexOf("const openEntryEditor"), root.indexOf("const browseExistingVault"));
    expect(navigation).toContain("if (details) details.open = false;");
    expect(navigation).toContain(".fleet-vault-grants select, .fleet-vault-grants button");
    expect(navigation).toContain('querySelector<HTMLElement>("input")');
    expect(navigation).toContain("target?.focus()");
    expect(root).not.toContain("key={editorMode}");
  });

  test("uses responsive tracks and wrapping without adding an overflow owner", () => {
    expect(css).toContain("repeat(3, minmax(0, 1fr))");
    expect(css).toContain("@container (max-width: 960px)");
    expect(css).toContain("@container (max-width: 600px)");
    expect(css).toContain("overflow-wrap: anywhere");
    expect(css).toContain(":focus-visible");
    expect(css).not.toMatch(/overflow(?:-[xy])?\s*:\s*(?:auto|scroll|hidden|clip)/);
    expect(editorSource).not.toContain("ScrollArea");
    // Real bounding-box and scroll-owner assertions live in the browser check.
  });
});

describe("Vault access state boundaries remain distinct", () => {
  test("saved verification is separate from caller authorization and mounted state", () => {
    expect(vaultPolicyVerification(status)?.title).toBe("Saved to Windows");
    expect(vaultMountGate({ authorized: undefined, entryResult: "applied", draftDirty: false }).disabledReason).toContain("not authorized");
    expect(vaultMountGate({ authorized, entryResult: "applied", draftDirty: false }).canMount).toBe(true);
    expect(authorized.mount_state).toBe("unmounted");
    expect(vaultMountResultLabel({ entry_id: entry.id, state: "mounted", presentation: "machine", drive_letter: null, reason: null })).toBe("Mounted for this Windows session");
  });

  test("an unconfirmed draft cannot become saved or authorized just by being displayed", () => {
    expect(root).toContain("const verification = draftDirty ? null : vaultPolicyVerification(status)");
    expect(root).toContain("Draft auto-saved on this PC — not yet applied to Windows.");
    const gate = vaultMountGate({ authorized: undefined, entryResult: undefined, draftDirty: true });
    expect(gate.canMount).toBe(false);
    expect(gate.disabledReason).toContain("not been applied yet");
    expect(vaultPolicyVerification({ ...status, validation_state: "never_applied" })).toBeNull();
  });

  test("degraded, unavailable and validation warnings remain outside optional help", () => {
    const warning = vaultPolicyVerification({ ...status, validation_state: "degraded", entries: [{ id: entry.id, result: "acl_readback_failed" }] });
    expect(warning?.tone).toBe("warning");
    expect(warning?.detail).toContain("Mounting is unavailable until this is fixed");
    expect(vaultMountGate({ authorized, entryResult: "acl_readback_failed", draftDirty: false }).canMount).toBe(false);
    const beforeRecovery = root.split('<details className="fleet-vault-advanced">')[0];
    expect(beforeRecovery).toContain("Vault settings could not be loaded yet");
    expect(beforeRecovery).toContain("Run WinCommander as administrator to change Vault settings");
    expect(beforeRecovery).toContain('className="fleet-validation-errors"');
    expect(beforeRecovery).toContain("{verification.detail}");
    expect(editorSource).not.toContain("useVaultAccess");
    expect(editorSource).not.toContain("localStorage");
    expect(editorSource).not.toContain("invoke(");
  });
});
