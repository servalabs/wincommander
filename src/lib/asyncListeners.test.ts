import { describe, expect, test } from "bun:test";
import { createAsyncListenerScope } from "./asyncListeners";

function deferredRegistration() {
  let callback: (event: string) => void = () => {};
  let resolve!: (unlisten: () => void) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<() => void>((accept, refuse) => { resolve = accept; reject = refuse; });
  return {
    register(handler: (event: string) => void) { callback = handler; return promise; },
    emit(event = "event") { callback(event); },
    resolve,
    reject,
  };
}

describe("asynchronous listener lifetime", () => {
  test("unregisters a listener that finishes registering after teardown exactly once", async () => {
    const registration = deferredRegistration();
    const scope = createAsyncListenerScope(() => {});
    let cleaned = 0;
    const pending = scope.listen(registration.register, () => {});
    scope.dispose();
    registration.resolve(() => { cleaned += 1; });
    await pending;
    expect(cleaned).toBe(1);
    scope.dispose();
    expect(cleaned).toBe(1);
  });

  test("delivers live events but ignores queued callbacks after teardown", async () => {
    const registration = deferredRegistration();
    const scope = createAsyncListenerScope(() => {});
    const received: string[] = [];
    const pending = scope.listen(registration.register, (event) => received.push(event));
    registration.emit("live");
    scope.dispose();
    registration.emit("queued");
    registration.resolve(() => {});
    await pending;
    registration.emit("late");
    expect(received).toEqual(["live"]);
  });

  test("cleans all registered listeners once, even when one cleanup throws", async () => {
    const errors: unknown[] = [];
    const scope = createAsyncListenerScope((error) => errors.push(error));
    const failure = new Error("cleanup failed");
    let cleaned = 0;
    await scope.listen(async () => () => { throw failure; }, () => {});
    await scope.listen(async () => () => { cleaned += 1; }, () => {});
    scope.dispose();
    scope.dispose();
    expect(cleaned).toBe(1);
    expect(errors).toEqual([failure]);
  });

  test("observes rejection after teardown without an unhandled promise", async () => {
    const registration = deferredRegistration();
    const errors: unknown[] = [];
    const scope = createAsyncListenerScope((error) => errors.push(error));
    const pending = scope.listen(registration.register, () => {});
    scope.dispose();
    const failure = new Error("registration failed");
    registration.reject(failure);
    await pending;
    expect(errors).toEqual([failure]);
  });

  test("observes asynchronous native cleanup rejection for live and late registration", async () => {
    for (const disposeBeforeRegistration of [false, true]) {
      const errors: unknown[] = [];
      const scope = createAsyncListenerScope((error) => errors.push(error));
      const failure = new Error("native cleanup rejected");
      let cleaned = 0;
      const pending = scope.listen(async () => async () => { cleaned += 1; throw failure; }, () => {});
      if (disposeBeforeRegistration) scope.dispose();
      await pending;
      scope.dispose();
      scope.dispose();
      await Promise.resolve();
      expect(cleaned).toBe(1);
      expect(errors).toEqual([failure]);
    }
  });

  test("handles synchronous registration failure and allows another live listener", async () => {
    const errors: unknown[] = [];
    const scope = createAsyncListenerScope((error) => errors.push(error));
    const failure = new Error("no window");
    await scope.listen(() => { throw failure; }, () => {});
    let cleaned = 0;
    await scope.listen(async () => () => { cleaned += 1; }, () => {});
    scope.dispose();
    expect(errors).toEqual([failure]);
    expect(cleaned).toBe(1);
  });

  test("does not start the next sequential registration after a pending registration is disposed", async () => {
    const registration = deferredRegistration();
    const scope = createAsyncListenerScope(() => {});
    const pending = scope.listen(registration.register, () => {});
    scope.dispose();
    registration.resolve(() => {});
    await pending;
    let started = false;
    await scope.listen(async () => { started = true; return () => {}; }, () => {});
    expect(started).toBe(false);
  });

  test("reports registration failure so a sequential owner can stop the sequence", async () => {
    const errors: unknown[] = [];
    const scope = createAsyncListenerScope((error) => errors.push(error));
    const failure = new Error("first listener failed");
    let nextStarted = false;
    const registerSequence = async () => {
      if (!await scope.listen(async () => { throw failure; }, () => {})) return;
      await scope.listen(async () => { nextStarted = true; return () => {}; }, () => {});
    };
    await registerSequence();
    expect(nextStarted).toBe(false);
    expect(errors).toEqual([failure]);
    scope.dispose();
  });

  test("reopening uses a fresh scope and cannot revive the closed screen", async () => {
    const oldRegistration = deferredRegistration();
    const newRegistration = deferredRegistration();
    const oldScope = createAsyncListenerScope(() => {});
    const newScope = createAsyncListenerScope(() => {});
    const received: string[] = [];
    const oldPending = oldScope.listen(oldRegistration.register, () => received.push("old"));
    oldScope.dispose();
    const newPending = newScope.listen(newRegistration.register, () => received.push("new"));
    oldRegistration.resolve(() => {});
    newRegistration.resolve(() => {});
    await Promise.all([oldPending, newPending]);
    oldRegistration.emit();
    newRegistration.emit();
    expect(received).toEqual(["new"]);
    newScope.dispose();
  });
});
