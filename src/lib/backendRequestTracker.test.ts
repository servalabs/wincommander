import { expect, test } from "bun:test";
import { createBackendRequestTracker } from "./backendRequestTracker";

test("explicit Vault status reads do not reuse an older in-flight probe", async () => {
  const run = createBackendRequestTracker();
  let finish!: (value: string) => void;
  const older = run("Get-EncryptedVolumeStatus", "inventory", () => new Promise<string>(resolve => { finish = resolve; }));
  expect(await run("Get-EncryptedVolumeStatus", "inventory", async () => "fresh")).toBe("fresh");
  finish("old");
  expect(await older).toBe("old");
});

test("duplicate mutations still share one operation and a completed operation may run again", async () => {
  const run = createBackendRequestTracker();
  let calls = 0;
  let finish!: (value: string) => void;
  const first = run("Dismount-EncryptedVolume", "J", () => { ++calls; return new Promise<string>(resolve => { finish = resolve; }); });
  const duplicate = run("Dismount-EncryptedVolume", "J", async () => { ++calls; return "wrong"; });
  expect(calls).toBe(1);
  finish("confirmed");
  expect(await first).toBe("confirmed");
  expect(await duplicate).toBe("confirmed");
  expect(await run("Dismount-EncryptedVolume", "J", async () => "new")).toBe("new");
});

test("a failed backend operation cannot wedge later retries", async () => {
  const run = createBackendRequestTracker();
  const failed = await run("example", "key", async () => { throw Error("failed"); }).catch(error => error.message);
  expect(failed).toBe("failed");
  expect(await run("example", "key", async () => "retried")).toBe("retried");
});
