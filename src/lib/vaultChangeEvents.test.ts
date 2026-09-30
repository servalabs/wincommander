import { expect, test } from "bun:test";
import { afterVaultMutation, FLEET_VAULTS_CHANGED_EVENT } from "./vaultChangeEvents";

test("all observers refresh after success, without receiving private vault data", async () => {
  const events = new EventTarget();
  const received: Event[] = [];
  events.addEventListener(FLEET_VAULTS_CHANGED_EVENT, event => received.push(event));
  const receipt = { state: "unmounted" };
  expect(await afterVaultMutation(async () => receipt, events)).toBe(receipt);
  expect(received).toHaveLength(1);
  expect("detail" in received[0]).toBe(false);
});

test("partial failure also refreshes observers while preserving the original error", async () => {
  const events = new EventTarget();
  let refreshes = 0;
  events.addEventListener(FLEET_VAULTS_CHANGED_EVENT, () => ++refreshes);
  const failure = new Error("partial dismount");
  const caught = await afterVaultMutation(async () => { throw failure; }, events).catch(error => error);
  expect(caught).toBe(failure);
  expect(refreshes).toBe(1);
});
