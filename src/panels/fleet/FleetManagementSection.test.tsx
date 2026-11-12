import { expect, test } from "bun:test";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): { stderr: ReadableStream<Uint8Array>; exited: Promise<number> };
};

test("management editors require an elevated process and service approval", async () => {
  const script = String.raw`
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    import { JSDOM } from 'jsdom';
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window; globalThis.document = dom.window.document;
    globalThis.navigator = dom.window.navigator; globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const React = await import('react'); const { act, createElement: h } = React;
    const { createRoot } = await import('react-dom/client');
    let elevation = 'standard', permission = false, capabilityCalls = 0, pending = null;
    const getCapabilities = async () => { capabilityCalls++; return pending ? new Promise(resolve => pending.push(resolve)) : { can_manage_policy: permission }; };
    const getAccessDirectory = async () => ({}), saveAccessDirectory = async () => ({});
    mock.module('@/hooks/useProcessElevation', () => ({ default: () => ({ state: elevation }) }));
    mock.module('@/hooks/useVaultAccess', () => ({ default: () => ({ getCapabilities, getAccessDirectory, saveAccessDirectory }) }));
    mock.module('./src/panels/fleet/useFleetAccessDirectory', () => ({ useFleetAccessDirectory: enabled => ({
      directory: { groups: [], users: [] }, loaded: enabled, loading: false, saving: false,
      dirty: false, update() {}, refresh() {}, save() {},
    }) }));
    mock.module('./src/panels/fleet/AccessControlTab', () => ({ default: () => h('div', null, 'ACCESS EDITOR') }));
    mock.module('./src/panels/fleet/VaultAccessTab', () => ({ default: () => h('div', null, 'VAULT EDITOR') }));
    const { default: Section } = await import('./src/panels/fleet/FleetManagementSection');
    const host = document.getElementById('root');
    async function view(section, elevated, allowed) {
      elevation = elevated; permission = allowed; const root = createRoot(host);
      await act(async () => { root.render(h(Section, { section })); });
      const text = host.textContent;
      await act(async () => { root.unmount(); });
      return text;
    }
    for (const [section, marker] of [['access-control', 'ACCESS EDITOR'], ['vault', 'VAULT EDITOR']]) {
      const before = capabilityCalls;
      assert.ok(!(await view(section, 'standard', true)).includes(marker));
      assert.equal(capabilityCalls, before, 'non-elevated process should not query admin capability');
      assert.ok(!(await view(section, 'administrator', false)).includes(marker));
      assert.ok((await view(section, 'administrator', true)).includes(marker));
    }
    const { useFleetManagementAccess } = await import('./src/panels/fleet/useFleetManagementAccess');
    const Gate = () => h('div', null, useFleetManagementAccess().canManage ? 'allowed' : 'blocked');
    elevation = 'administrator'; pending = []; const root = createRoot(host);
    await act(async () => { root.render(h(Gate)); });
    await act(async () => { window.dispatchEvent(new window.Event('focus')); });
    assert.equal(pending.length, 2);
    await act(async () => { pending[1]({ can_manage_policy: false }); });
    assert.equal(host.textContent, 'blocked');
    await act(async () => { pending[0]({ can_manage_policy: true }); });
    assert.equal(host.textContent, 'blocked', 'stale approval must not override newer denial');
    await act(async () => { root.unmount(); });
    dom.window.close();
  `;
  const child = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const stderr = await new Response(child.stderr).text();
  expect({ status: await child.exited, stderr }).toEqual({ status: 0, stderr: "" });
});
