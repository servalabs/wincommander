import { test, expect } from "bun:test";

// Models the serialize semantics: each patch awaits the previous, so writes apply
// in order and the last one wins — none is lost.
test("serialized chain applies writes in order, last wins", async () => {
  let chain: Promise<unknown> = Promise.resolve();
  const applied: number[] = [];
  const patch = (n: number) => {
    chain = chain.then(async () => { await Promise.resolve(); applied.push(n); });
    return chain;
  };
  patch(1); patch(2); await patch(3);
  expect(applied).toEqual([1, 2, 3]);
});

test("a throwing write does not break the chain", async () => {
  let chain: Promise<unknown> = Promise.resolve();
  const applied: number[] = [];
  const patch = (n: number, fail = false) => {
    const run = chain.then(async () => { if (fail) throw new Error("x"); applied.push(n); });
    chain = run.catch(() => {});
    return run;
  };
  patch(1);
  await patch(2, true).catch(() => {});
  await patch(3);
  expect(applied).toEqual([1, 3]);
});

// Regression coverage for the VisibilityTable / LockdownConfigSection toggle
// bug: rapid clicks on two DIFFERENT rows that share a collection field (an
// object like selfDestruct.steps, or an array like lockedPanelIds) each
// capture that field from the component's render-time state before either
// write resolves. A plain-object patch bakes in that stale snapshot at call
// time; an updater function `(latest) => patch` (patchAppSettings' new form)
// defers reading the snapshot until this write's turn in the chain, so it
// sees the OTHER write's result once that write has landed.

test("stale full-object patch built before either write resolves clobbers a sibling's in-flight write", async () => {
  let latest: { steps: Record<string, boolean> } = { steps: { a: false, b: false } };
  let chain: Promise<unknown> = Promise.resolve();
  const patch = (p: { steps: Record<string, boolean> }) => {
    const run = chain.then(async () => {
      await Promise.resolve(); // simulate the IPC round trip
      latest = { steps: { ...latest.steps, ...p.steps } };
    });
    chain = run.catch(() => {});
    return run;
  };

  // Both "clicks" read `steps` from the SAME pre-write snapshot — exactly what
  // a component's `useMemo(() => config?.steps, [config?.steps])` returns when
  // a second toggle is clicked before the first toggle's write has resolved
  // and re-rendered the component.
  const staleSteps = latest.steps;
  const p1 = patch({ steps: { ...staleSteps, a: true } });
  const p2 = patch({ steps: { ...staleSteps, b: true } });
  await Promise.all([p1, p2]);

  // Bug: p2's patch re-sent a's OLD value explicitly, so the deep-merged
  // write silently reverted the toggle the user had just switched on.
  expect(latest.steps).toEqual({ a: false, b: true });
});

test("updater-function patch resolved at write time avoids clobbering a sibling's in-flight write", async () => {
  let latest: { steps: Record<string, boolean> } = { steps: { a: false, b: false } };
  let chain: Promise<unknown> = Promise.resolve();
  const patch = (p: { steps: Record<string, boolean> } | ((l: typeof latest) => { steps: Record<string, boolean> })) => {
    const run = chain.then(async () => {
      const resolved = typeof p === "function" ? p(latest) : p;
      await Promise.resolve(); // simulate the IPC round trip
      latest = { steps: { ...latest.steps, ...resolved.steps } };
    });
    chain = run.catch(() => {});
    return run;
  };

  // Same two rapid "clicks" as above, but each sends only its own key via an
  // updater — resolved against `latest` at its turn in the chain instead of
  // a snapshot captured before either write ran.
  const p1 = patch(() => ({ steps: { a: true } }));
  const p2 = patch(() => ({ steps: { b: true } }));
  await Promise.all([p1, p2]);

  expect(latest.steps).toEqual({ a: true, b: true });
});
