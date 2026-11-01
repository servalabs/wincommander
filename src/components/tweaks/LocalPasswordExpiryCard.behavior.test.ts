import { expect, test } from "bun:test";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): {
    stderr: ReadableStream<Uint8Array>;
    exited: Promise<number>;
  };
};

test("password expiry change requires confirmation and preserves the selected direction", async () => {
  const script = `
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    const react = await import('react');
    const status = { totalEligible: 2, allEligibleNeverExpire: false, isAdmin: true, skipped: {} };
    let stateIndex = 0, resolveConfirm, resolveMutation;
    const confirmations = [], calls = [];
    mock.module('react', () => ({ ...react,
      useState: initial => [stateIndex++ === 0 ? status : initial, () => {}],
      useCallback: fn => fn, useMemo: fn => fn(), useEffect: () => {}, useRef: value => ({ current: value }),
    }));
    mock.module('./src/components/shared/AppConfirmDialog', () => ({ useAppConfirm: () => request => { confirmations.push(request); return new Promise(resolve => { resolveConfirm = resolve; }); } }));
    mock.module('./src/hooks/useBackend', () => ({ default: () => ({}), executeBackendCommand: async (command, args) => { calls.push({ command, args }); if (command === 'Get-LocalPasswordExpiryStatus') return { success: true, data: status }; return new Promise(resolve => { resolveMutation = resolve; }); } }));
    mock.module('./src/components/tweaks/localPasswordExpiryUtils', () => ({ formatPasswordExpirySummary: () => '', formatSkippedPasswordExpiryAccounts: () => '' }));
    const toast = await import('./src/utils/toast');
    mock.module('./src/utils/toast', () => ({ ...toast, showSuccess: () => {} }));
    const { default: Card } = await import('./src/components/tweaks/LocalPasswordExpiryCard');
    const tree = Card();
    function find(node) {
      if (!node || typeof node !== 'object') return;
      if (node.props?.['aria-label'] === 'Set password never expires for eligible local accounts') return node;
      for (const child of react.Children.toArray(node.props?.children)) { const found = find(child); if (found) return found; }
    }
    const toggle = find(tree); assert.ok(toggle);
    const event = checked => ({ currentTarget: { checked } });
    toggle.props.onChange(event(true));
    toggle.props.onChange(event(true));
    assert.equal(confirmations.length, 1);
    assert.match(confirmations[0].description, /all 2 eligible local accounts/);
    resolveConfirm(false);
    await new Promise(resolve => setTimeout(resolve, 0));
    assert.equal(calls.length, 0);
    toggle.props.onChange(event(false));
    assert.match(confirmations[1].description, /does not restore each account's previous setting/);
    resolveConfirm(true);
    await new Promise(resolve => setTimeout(resolve, 0));
    assert.deepEqual(calls[0], { command: 'Set-LocalPasswordNeverExpires', args: { Enabled: false } });
    toggle.props.onChange(event(true));
    assert.equal(confirmations.length, 2);
    assert.equal(calls.length, 1);
    resolveMutation({ success: true, data: { status, changedCount: 2, failedCount: 0 } });
    await new Promise(resolve => setTimeout(resolve, 0));
    toggle.props.onChange(event(true));
    assert.equal(confirmations.length, 3);
    resolveConfirm(true);
    await new Promise(resolve => setTimeout(resolve, 0));
    resolveMutation({ success: false, error: 'denied' });
    await new Promise(resolve => setTimeout(resolve, 0));
    assert.equal(calls.at(-1).command, 'Get-LocalPasswordExpiryStatus');
    toggle.props.onChange(event(false));
    assert.equal(confirmations.length, 4);
    resolveConfirm(false);
    await new Promise(resolve => setTimeout(resolve, 0));
  `;
  const process = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const stderr = await new Response(process.stderr).text();
  expect({ code: await process.exited, stderr }).toEqual({ code: 0, stderr: "" });
});
