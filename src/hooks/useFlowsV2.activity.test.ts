import { expect, test } from "bun:test";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): {
    stderr: ReadableStream<Uint8Array>;
    exited: Promise<number>;
  };
};

test("Flow activity listener distinguishes incomplete, completed and older unknown receipts", async () => {
  const script = `
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    const react = await import('react');
    let state;
    const diagnostics = [];
    const listeners = new Map();
    mock.module('react', () => ({ ...react,
      useState: initial => { state = initial; return [initial, update => { state = typeof update === 'function' ? update(state) : update; }]; },
      useRef: value => ({ current: value }),
      useCallback: callback => callback,
      useEffect: effect => { effect(); },
    }));
    mock.module('@tauri-apps/api/core', () => ({ invoke: async () => [] }));
    mock.module('@tauri-apps/api/event', () => ({ listen: async (event, callback) => { listeners.set(event, callback); return () => {}; } }));
    mock.module('./src/lib/diagnostics', () => ({ newDiagnosticOperationId: () => 'test', recordDiagnostic: event => { diagnostics.push(event); } }));
    const { useFlowsV2 } = await import('./src/hooks/useFlowsV2');
    useFlowsV2(true);
    for (let i = 0; i < 8; i++) await Promise.resolve();
    const emit = listeners.get('flow-executed');
    assert.ok(emit);
    emit({ payload: { ruleId: 'r1', completed: false, failedActionCount: 2 } });
    assert.equal(state.log[0].kind, 'incomplete');
    assert.match(state.log[0].message, /2 action\\(s\\) failed or returned an unconfirmed result/);
    emit({ payload: { ruleId: 'r1', completed: true, failedActionCount: 0 } });
    assert.equal(state.log[0].kind, 'executed');
    assert.match(state.log[0].message, /reported completed/);
    emit({ payload: { ruleId: 'r1', actionCount: 3 } });
    assert.equal(state.log[0].kind, 'unknown');
    assert.match(state.log[0].message, /outcome unknown/);
    emit({ payload: { ruleId: 'r1', completed: null, failedActionCount: 0 } });
    assert.equal(state.log[0].kind, 'unknown');
    emit({ payload: { flowId: 'r1', dryRun: true, completed: true } });
    assert.equal(state.log[0].kind, 'simulated');
    assert.match(state.log[0].message, /Simulation ended/);
    emit({ payload: { flowId: 'r1', completed: false, steps: [] } });
    assert.equal(state.log[0].kind, 'incomplete');
    assert.match(state.log[0].message, /incomplete or unconfirmed/);
    const { default: FlowActivityLogger } = await import('./src/components/FlowActivityLogger');
    FlowActivityLogger();
    const logEvent = listeners.get('flow-executed');
    logEvent({ payload: { dryRun: true, completed: true } });
    assert.equal(diagnostics.at(-1).outcome, 'progress');
    assert.equal(diagnostics.at(-1).context.state, 'simulated');
    logEvent({ payload: { completed: null, failedActionCount: 0 } });
    assert.equal(diagnostics.at(-1).outcome, 'progress');
    assert.equal(diagnostics.at(-1).context.state, 'unknown');
    logEvent({ payload: { completed: false, steps: [] } });
    assert.equal(diagnostics.at(-1).outcome, 'degraded');
    assert.equal(diagnostics.at(-1).context.state, 'incomplete');
  `;
  const process = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const stderr = await new Response(process.stderr).text();
  expect({ code: await process.exited, stderr }).toEqual({ code: 0, stderr: "" });
});
