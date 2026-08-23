import { describe, expect, test } from "bun:test";
import { createStartupProbeStore } from "./startupProbeStore";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe("startup probe store", () => {
  test("shares one authoritative probe result with all launch consumers", async () => {
    const store = createStartupProbeStore(() => 42);
    const pending = deferred<{ secure: boolean }>();
    let calls = 0;
    const controller = new AbortController();
    const load = async () => {
      calls++;
      return pending.promise;
    };

    const first = store.refresh(load, controller.signal);
    const second = store.refresh(load, controller.signal);
    pending.resolve({ secure: true });

    expect(await first).toEqual({ secure: true });
    expect(await second).toEqual({ secure: true });
    expect(calls).toBe(1);
    expect(store.getSnapshot()).toEqual({
      value: { secure: true },
      refreshedAt: 42,
      isRefreshing: false,
    });
  });
});
