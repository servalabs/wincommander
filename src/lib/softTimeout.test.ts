import { describe, expect, test } from "bun:test";
import { waitForSoftTimeout } from "./softTimeout";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("soft timeout", () => {
  test("returns completed when the operation finishes before the timer", async () => {
    const result = await waitForSoftTimeout(Promise.resolve("ready"), 50);

    expect(result).toEqual({ status: "completed", value: "ready" });
  });

  test("returns timed-out without cancelling the underlying operation", async () => {
    const pending = deferred<string>();

    const result = await waitForSoftTimeout(pending.promise, 5);

    expect(result).toEqual({ status: "timed-out" });

    pending.resolve("eventual-result");
    await expect(pending.promise).resolves.toBe("eventual-result");
  });
});
