import { expect, test } from "bun:test";

declare const Bun: { which(name: string): string | null; spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): { stderr: ReadableStream<Uint8Array>; exited: Promise<number>; kill(): void } };

// Each script runs in its own process so its module mocks and JSDOM globals never reach other tests.
async function runIsolated(body: string) {
  const script = String.raw`
    import { mock } from 'bun:test'; import { strict as assert } from 'node:assert'; import { JSDOM } from 'jsdom';
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window; globalThis.document = dom.window.document; globalThis.navigator = dom.window.navigator;
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    globalThis.ResizeObserver = window.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} };
    // Radix creates and dispatches its own events, which JSDOM accepts only from its own constructors.
    for (const name of ['Event', 'CustomEvent', 'FocusEvent', 'MouseEvent', 'KeyboardEvent', 'MutationObserver', 'HTMLElement', 'Element', 'Node']) globalThis[name] = window[name];
    globalThis.getComputedStyle = window.getComputedStyle.bind(window);
    const React = await import('react'); const { act, createElement: h } = React; const { createRoot } = await import('react-dom/client');
    const host = document.getElementById('root');
    const openMenu = async trigger => { await act(async () => { trigger.dispatchEvent(new window.KeyboardEvent('keydown', { key: 'Enter', bubbles: true })); }); };
    const menuItems = () => [...document.querySelectorAll('[role="menuitem"]')];
  ` + body;
  const child = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const timer = setTimeout(() => child.kill(), 30_000);
  try { const stderr = await new Response(child.stderr).text(); expect({ code: await child.exited, stderr }).toEqual({ code: 0, stderr: "" }); }
  finally { clearTimeout(timer); }
}

test("the vault-file More button looks like the Vault Permissions one and opens both dialogs for a file the person picks", async () => {
  await runIsolated(String.raw`
    const opened = [];
    const actions = { changePassword: target => opened.push(['password', target]), rename: target => opened.push(['rename', target]) };
    const { VaultFileMoreMenu } = await import('./src/panels/vault/VaultFileActionButtons');
    const { default: VaultPolicyMoreMenu } = await import('./src/panels/fleet/VaultPolicyMoreMenu');
    const root = createRoot(host);
    await act(async () => root.render(h('div', null, h(VaultFileMoreMenu, { actions }), h(VaultPolicyMoreMenu, { label: 'Shared', items: [] }))));
    const trigger = host.querySelector('[aria-label="More vault actions"]');
    const policyTrigger = host.querySelector('[aria-label="More actions for Shared"]');
    assert.ok(trigger && policyTrigger);
    assert.equal(trigger.getAttribute('title'), 'More vault actions');
    assert.equal(trigger.textContent, '');
    assert.deepEqual(trigger.className.replace(' vault-card-more', ''), policyTrigger.className);
    assert.equal(trigger.innerHTML, policyTrigger.innerHTML);
    for (const [label, dialog] of [['Change Password…', 'password'], ['Rename…', 'rename']]) {
      await openMenu(trigger);
      assert.deepEqual(menuItems().map(item => item.textContent), ['Change Password…', 'Rename…']);
      assert.ok(menuItems().every(item => !item.hasAttribute('data-disabled')));
      await act(async () => { menuItems().find(item => item.textContent === label).click(); });
      assert.deepEqual(opened.pop(), [dialog, { path: null }]);
      assert.equal(menuItems().length, 0);
    }
    await act(async () => root.unmount()); dom.window.close();
  `);
}, 40_000);

test("Encrypted Volumes keeps Change Password and Rename only in the ⋯ at the right end of its Pro action row", async () => {
  await runIsolated(String.raw`
    let paid = true;
    const opened = [];
    const fileActions = { changePassword: target => opened.push(['password', target]), rename: target => opened.push(['rename', target]) };
    const volumes = [{ letter: 'V:', path: 'C:\\Vaults\\records.hc', type: 'Normal', accessible: true, dismountAllowed: true }];
    const stub = name => () => ({ default: () => h('div', null, name) });
    mock.module('./src/context/AppContext', () => ({ useAppState: () => ({ encryptionStatus: { volumes }, refreshVault: async () => null, loading: { vault: false }, vaultStatusError: null, vaultManualRefreshing: false }), useOptionalAppState: () => null }));
    mock.module('./src/context/ThemeContext', () => ({ useTheme: () => ({ theme: 'dark' }) }));
    mock.module('./src/hooks/useBackend', () => ({ default: () => ({}) }));
    mock.module('./src/hooks/useEntitlements', () => ({ default: () => ({ canUse: () => paid, isLoading: false, hasPaid: paid }) }));
    mock.module('./src/hooks/useVaultFileActions', () => ({ default: () => ({ actions: paid ? fileActions : null, dialogs: null }) }));
    mock.module('./src/hooks/useVaultNames', () => ({ default: () => ({ names: [] }) }));
    mock.module('./src/hooks/useVaultDriveLetters', () => ({ default: () => ({ letters: ['Y'], loading: false, unavailable: false, refresh: async () => {} }) }));
    mock.module('./src/hooks/useProcessElevation', () => ({ default: () => ({ state: 'standard' }) }));
    mock.module('./src/panels/vault/VolumeActionsMenu', () => ({ default: ({ letter }) => h('button', null, 'Dismount ' + letter) }));
    for (const name of ['CreateVolumeWizard', 'SystemEncryptionSection', 'RamDisksSection', 'BackupRecoverySection', 'CloudBackupSection', 'StegoBackupSection'])
      mock.module('./src/panels/vault/' + name, stub(name));
    mock.module('./src/panels/fleet/FleetManagementSection', stub('FleetManagementSection'));
    mock.module('./src/panels/fleet/VaultAccessTab', stub('VaultAccessTab'));
    const { default: VaultPanel } = await import('./src/panels/vault/index');
    const root = createRoot(host);
    await act(async () => root.render(h(VaultPanel)));
    const row = host.querySelector('.vault-card-action-row');
    const labels = [...row.children].map(node => node.getAttribute('aria-label') || node.textContent.trim());
    assert.deepEqual(labels, ['Create Volume', 'Mount Volume', 'More vault actions']);
    assert.doesNotMatch(host.textContent, /Other vault file/);
    assert.equal(host.querySelectorAll('[aria-label="More vault actions"]').length, 1);
    assert.doesNotMatch(host.querySelector('.encrypted-volumes-table').textContent, /Password|Rename/);
    await openMenu(row.lastElementChild);
    await act(async () => { menuItems().find(item => item.textContent === 'Rename…').click(); });
    assert.deepEqual(opened, [['rename', { path: null }]]);
    paid = false;
    await act(async () => root.render(h(VaultPanel, { key: 'free' })));
    assert.equal(host.querySelector('[aria-label="More vault actions"]'), null);
    assert.match(host.querySelector('.vault-card-action-row').textContent, /Unlock with WinCommander Pro/);
    await act(async () => root.unmount()); dom.window.close();
  `);
}, 40_000);
