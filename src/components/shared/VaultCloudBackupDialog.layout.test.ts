import { expect, test } from "bun:test";

declare const Bun: { which(name: string): string | null; spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): { stderr: ReadableStream<Uint8Array>; exited: Promise<number>; kill(): void } };

test("backup dialog separates current checks, historical results and controls", async () => {
  const script = String.raw`
    import { mock } from 'bun:test'; import { strict as assert } from 'node:assert'; import { JSDOM } from 'jsdom';
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window; globalThis.document = dom.window.document; globalThis.navigator = dom.window.navigator; globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const React = await import('react'); const { act, createElement: h } = React; const { createRoot } = await import('react-dom/client');
    let closed = 0; const actions = [];
    const result = { mount_receipt:'a'.repeat(64), engine_ready:true, connected:true, configured:true, running:true, watching:true, can_configure:true, configuration:null, connection_check:'connected', remote_check:'missing', last_run:null, last_backup:null, last_verification:null, last_restore:null };
    mock.module('./src/hooks/useVaultCloudBackup', () => ({ default: () => ({ result, busy:false, action:null, error:'', perform: op => { actions.push(op); } }) }));
    mock.module('@/components/ui/bp', () => ({ Button: ({children, icon, intent, minimal, ...props}) => h('button', props, children), Dialog: ({children}) => h('div', null, children), DialogBody: ({children}) => h('div', null, children), DialogFooter: ({actions}) => h('footer', null, actions) }));
    mock.module('@/components/ui/input', () => ({ Input: props => h('input', props) }));
    const {default: Dialog} = await import('./src/components/shared/VaultCloudBackupDialog');
    const root = createRoot(document.getElementById('root')); await act(async () => root.render(h(Dialog, { isOpen:true, onClose:() => closed++, internalDrive:4, driveLabel:'E:', personal:true })));
    const host = document.getElementById('root'); const btn = label => [...host.querySelectorAll('button')].find(x => x.textContent === label);
    assert.match(host.textContent, /Not checked during operation/); assert.doesNotMatch(host.textContent, /Not found on last check/); assert.match(host.textContent, /earlier checks/); assert.ok(host.querySelector('.vault-cloud-backup__spinner'));
    assert.equal(btn('Stop backup').disabled, false); await act(async () => btn('Stop backup').click()); assert.deepEqual(actions, ['stop']);
    result.running=false; await act(async () => root.render(h(Dialog, { isOpen:true, onClose:() => closed++, internalDrive:4, driveLabel:'E:', personal:true }))); assert.match(host.textContent, /Not found on last check/);
    await act(async () => btn('Settings').click()); assert.equal(btn('Settings').getAttribute('aria-pressed'), 'true');
    await act(async () => btn('Close').click()); assert.equal(closed, 1); assert.deepEqual(actions, ['stop']);
    await act(async () => root.unmount()); dom.window.close();
  `;
  const child = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const timer = setTimeout(() => child.kill(), 30_000);
  try { const stderr = await new Response(child.stderr).text(); expect({ code: await child.exited, stderr }).toEqual({ code: 0, stderr: "" }); }
  finally { clearTimeout(timer); }
}, 40_000);
