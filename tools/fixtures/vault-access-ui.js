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
  const entry = {
    id: 'example-vault', label: 'Example vault', container_path: '',
    container_kind: state === 'dual' ? 'dual' : 'standard', owner_account: 'ExampleUser',
    grants: [
      { principal_name: 'ExampleTeam', access: 'read' },
      { principal_name: 'ExampleUser', access: 'write' },
      { principal_name: 'ExampleReader', access: 'read' },
    ],
    mount: { presentation: 'machine' },
  };
  const policy = { schema_version: 1, policy_id: 'example-policy', version: 1, expected_previous_version: 0, entries: [entry] };
  const status = {
    policy_id: policy.policy_id, version: 1, applied_at: 1,
    validation_state: state === 'degraded' ? 'degraded' : 'current',
    entries: [{ id: entry.id, result: state === 'degraded' ? 'acl_readback_failed' : 'applied' }],
  };
  const authorized = {
    entry_id: entry.id, label: entry.label, access: 'write', presentation: 'machine',
    container_kind: entry.container_kind, mount_state: state === 'mounted' ? 'mounted' : 'unmounted', drive_letter: null,
  };
  const blocked = async () => { throw new Error('Native mutation blocked by UI fixture'); };
  window.__vaultUiService = {
    getPolicy: async () => {
      if (state === 'unavailable') throw new Error('Synthetic unavailable state');
      return structuredClone(policy);
    },
    getStatus: async () => structuredClone(status),
    getCapabilities: async () => ({ can_manage_policy: state !== 'unelevated' }),
    listAuthorizedEntries: async () => state === 'unauthorized' ? [] : [structuredClone(authorized)],
    applyPolicy: blocked, mountEntry: blocked, unmountEntry: blocked,
  };
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
    React.createElement(VaultAccessTab, { isAdmin: true, directory })));
}
