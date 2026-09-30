// Browser-only fixture: no actual Vault, path, credential or native mutation.
// Not imported by the application. The Playwright check supplies the service hook.
import React from 'react';
import { createRoot } from 'react-dom/client';
import '../../src/index.css';
import '../../src/styles/v2-theme.css';
import '../../src/panels/fleet/index.css';
import VaultAccessTab from '../../src/panels/fleet/VaultAccessTab';
import { readVaultAccessDraftSnapshot, writeVaultAccessDraft } from '../../src/panels/fleet/vaultAccessDraft';

const style = document.createElement('style');
style.textContent = 'html,body,#fixture{height:100%;margin:0} #fixture{overflow-y:auto} .fixture-panel{min-height:100%;height:auto;padding:16px;box-sizing:border-box}';
document.head.append(style);
let root;

export function renderVaultFixture(state) {
  root?.unmount();
  localStorage.clear();
  const standardOwner = state.startsWith('standard-owner');
  const degraded = state === 'degraded' || state === 'standard-owner-degraded';
  const callerSid = standardOwner ? 'S-1-5-21-fixture-standard' : 'S-1-5-21-fixture-owner';
  const sameOwnerMountedSibling = state === 'apply-sibling';
  let mounted = state === 'mounted';
  const entry = {
    id: 'example-vault', label: 'Example vault', container_path: sameOwnerMountedSibling || standardOwner ? 'C:\\Vaults\\existing-unmounted.vc' : '',
    container_kind: state === 'dual' ? 'dual' : 'standard', owner_account: standardOwner ? 'Example standard user' : 'ExampleUser',
    primary_owner_sid: callerSid,
    grants: standardOwner ? [{ principal_name: 'Example standard user', access: 'write' }] : [
      { principal_name: 'ExampleTeam', access: 'read' },
      { principal_name: 'ExampleUser', access: 'write' },
      { principal_name: 'ExampleReader', access: 'read' },
    ],
    mount: { presentation: standardOwner ? 'per-user' : 'machine' },
  };
  const ownerEntry = {
    entry,
    container_path_state: 'available',
    canonical_container_path: 'C:\\Vaults\\Example.vc',
    can_edit_policy: state !== 'outsider',
    can_remove_policy: true,
  };
  const mountedSibling = sameOwnerMountedSibling ? {
    ...entry,
    id: 'mounted-same-owner-sibling',
    label: 'Mounted same-owner sibling',
    container_path: 'C:\\Vaults\\mounted-sibling.vc',
    mount: { presentation: 'machine', preferred_letter: 'V' },
  } : null;
  const siblingOwnerEntry = mountedSibling && {
    entry: mountedSibling,
    container_path_state: 'available',
    canonical_container_path: mountedSibling.container_path,
    can_edit_policy: true,
    can_remove_policy: true,
  };
  let policy = { schema_version: 1, policy_id: 'example-policy', version: 1, expected_previous_version: 0, entries: [...(mountedSibling ? [entry, mountedSibling] : [entry])] };
  let fragment = { schema_version: 1, policy_id: policy.policy_id, version: 1, expected_previous_version: 0, entries: [...(siblingOwnerEntry ? [ownerEntry, siblingOwnerEntry] : [ownerEntry])] };
  const telemetry = { driveLetterRequests: [], appliedFragments: [] };
  const status = () => ({
    policy_id: policy.policy_id, version: 1, applied_at: 1,
    validation_state: degraded ? 'degraded' : 'current',
    entries: policy.entries.map(current => ({ id: current.id, result: degraded ? 'acl_readback_failed' : 'applied', mount_state: (current.id === entry.id && mounted) || current.id === mountedSibling?.id ? 'mounted' : 'unmounted' })),
  });
  const authorized = () => ({
    entry_id: entry.id, label: entry.label, access: 'write', presentation: 'machine',
    container_kind: entry.container_kind, mount_state: mounted ? 'mounted' : 'unmounted', drive_letter: null,
  });
  const blocked = async () => { throw new Error('Native mutation blocked by UI fixture'); };
  window.__vaultUiService = {
    getOwnerPolicyFragment: async () => {
      if (state === 'unavailable' || state === 'standard-owner-policy-unavailable') throw new Error('Synthetic unavailable state');
      return structuredClone(fragment);
    },
    getStatus: async () => {
      if (state === 'standard-owner-status-unavailable') throw new Error('Synthetic status unavailable');
      return structuredClone(status());
    },
    getCapabilities: async () => ({ can_manage_policy: state !== 'unelevated' && !standardOwner }),
    listAuthorizedEntries: async () => state === 'unauthorized' ? [] : [structuredClone(authorized())],
    listOwnerPrincipals: async () => ({ current_caller_sid: callerSid, principals: [
      { sid: callerSid, display_name: 'Example user', is_local_administrator: true },
      { sid: 'S-1-5-21-fixture-standard', display_name: 'Example standard user', is_local_administrator: false },
      { sid: 'S-1-5-21-fixture-admin', display_name: 'Example administrator', is_local_administrator: true },
    ] }),
    applyOwnerPolicyFragment: async submitted => {
      telemetry.appliedFragments.push(structuredClone(submitted));
      const replacements = new Map(submitted.entries.map(row => [row.entry.id, row.entry]));
      const additions = submitted.entries.map(row => row.entry).filter(added => !policy.entries.some(existing => existing.id === added.id));
      policy = { ...policy, version: submitted.version, expected_previous_version: submitted.expected_previous_version, entries: [...policy.entries.map(existing => replacements.get(existing.id) ?? existing), ...additions] };
      fragment = {
        ...fragment,
        version: policy.version,
        expected_previous_version: policy.expected_previous_version,
        entries: [
          ...fragment.entries.map(row => replacements.has(row.entry.id) ? { ...row, entry: replacements.get(row.entry.id) } : row),
          ...additions.map(added => ({ entry: added, container_path_state: 'available', canonical_container_path: added.container_path, can_edit_policy: added.primary_owner_sid === callerSid, can_remove_policy: true })),
        ],
      };
      window.__fixtureLatestFragment = structuredClone(fragment);
      return structuredClone(status());
    },
    forgetPolicy: blocked, mountEntry: blocked, unmountEntry: blocked,
  };
  window.__vaultBackend = {
    getAvailableDriveLetters: async entryId => {
      telemetry.driveLetterRequests.push(entryId ?? null);
      return { success: true, data: { letters: ['J:', 'k', 'W'] } };
    },
    openEncryptionVolume: blocked,
  };
  window.__fixtureVaultTelemetry = telemetry;
  window.__fixtureResetVaultTelemetry = () => {
    telemetry.driveLetterRequests.length = 0;
    telemetry.appliedFragments.length = 0;
  };
  window.__fixtureSetMounted = value => { mounted = Boolean(value); };
  if (state === 'draft') {
    writeVaultAccessDraft({ ...policy, entries: [{ ...entry, label: 'Example draft' }] }, undefined, policy);
    if (readVaultAccessDraftSnapshot()?.policy.entries[0]?.label !== 'Example draft') {
      throw new Error('Synthetic recovery draft did not round-trip through its existing storage API');
    }
  }
  const directory = {
    schema: 1,
    users: [
      { id: 'example-user', username: 'ExampleUser', displayName: 'Example user' },
      { id: 'example-reader', username: 'ExampleReader', displayName: 'Example reader' },
    ],
    groups: [{ id: 'example-group', name: 'Example team', localGroup: 'ExampleTeam', userIds: [] }],
  };
  root = createRoot(document.getElementById('fixture'));
  root.render(React.createElement('div', { className: 'panel-container fleet-panel fixture-panel' },
    React.createElement(VaultAccessTab, { isAdmin: !standardOwner, directory })));
}
