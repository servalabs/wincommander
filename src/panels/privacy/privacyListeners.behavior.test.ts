import { expect, test } from 'bun:test';

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): {
    stderr: ReadableStream<Uint8Array>;
    exited: Promise<number>;
  };
};

test('privacy security listeners own delayed registration and retain active events', async () => {
  const script = `
    import {mock} from 'bun:test';
    import {strict as assert} from 'node:assert';
    const actualReact = await import('react');
    let effects = [], updates = 0, invokeCount = 0, successes = 0, callbacks = [], resolves = [], disposed = 0;
    mock.module('react', () => ({...actualReact,
      useState: initial => [initial, () => { updates++; }],
      useCallback: callback => callback,
      useEffect: effect => effects.push(effect),
    }));
    mock.module('@tauri-apps/api/core', () => ({invoke: async (name) => { invokeCount++; if (name.includes('recent') || name.includes('list')) return []; return {}; }}));
    mock.module('@tauri-apps/api/event', () => ({listen: (name, callback) => { callbacks.push({name,callback}); return new Promise(resolve => resolves.push(resolve)); }}));
    mock.module('@tauri-apps/plugin-dialog', () => ({save: async () => null}));
    mock.module('./src/components/ui/bp', () => ({Button:'Button', HTMLSelect:'HTMLSelect', InputGroup:'InputGroup', Spinner:'Spinner', Tag:'Tag'}));
    mock.module('./src/components/shared/SectionCard', () => ({default:'SectionCard'}));
    mock.module('./src/components/shared/TierGate', () => ({default:'TierGate'}));
    mock.module('./src/components/shared/AppConfirmDialog', () => ({useAppConfirm: () => async () => false}));
    mock.module('./src/hooks/useEntitlements', () => ({default: () => ({hasPaid:true})}));
    mock.module('./src/panels/privacy/PrivacyEventTable', () => ({default:'PrivacyEventTable'}));
    mock.module('./src/utils/toast', () => ({showSuccess: () => { successes++; }, showError: () => {}}));
    const {default: Canary} = await import('./src/panels/privacy/CanaryTokensSection');
    const {default: Capture} = await import('./src/panels/privacy/useScreenCapture');
    const settle = async () => {for(let i=0;i<10;i++) await Promise.resolve();};
    async function check(render, expected, payload) {
      effects = []; callbacks = []; resolves = [];
      render(); const cleanups = effects.map(e => e()).filter(Boolean); await settle();
      assert.equal(callbacks.length, 1); assert.equal(callbacks[0].name, expected);
      let beforeUpdates = updates, beforeInvokes = invokeCount, beforeSuccess = successes;
      callbacks[0].callback({payload}); await settle();
      assert.ok(updates > beforeUpdates, 'active event updates state');
      if (expected === 'canary-token-fired') assert.equal(successes, beforeSuccess + 1);
      else assert.ok(invokeCount > beforeInvokes, 'active capture event refreshes status');
      cleanups.forEach(c => c()); beforeUpdates = updates; beforeInvokes = invokeCount; beforeSuccess = successes;
      callbacks[0].callback({payload}); await settle();
      assert.equal(updates,beforeUpdates); assert.equal(invokeCount,beforeInvokes); assert.equal(successes,beforeSuccess);
      const beforeDisposed = disposed; resolves[0](() => {disposed++;}); await settle();
      assert.equal(disposed,beforeDisposed+1, 'late registration disposed exactly once');
      callbacks[0].callback({payload}); await settle();
      assert.equal(updates,beforeUpdates); assert.equal(invokeCount,beforeInvokes); assert.equal(successes,beforeSuccess);
    }
    await check(() => Canary(), 'canary-token-fired', {label:'fixture',tokenId:'id'});
    await check(() => Capture({}, () => {}), 'screen-capture-detected', {tool:'fixture'});
  `;
  const proc = Bun.spawn([Bun.which('bun')!, '-e', script], {stdout:'pipe',stderr:'pipe'});
  const stderr = await new Response(proc.stderr).text();
  expect({code:await proc.exited, stderr}).toEqual({code:0,stderr:''});
});
