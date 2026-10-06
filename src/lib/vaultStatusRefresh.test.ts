import { expect, test } from "bun:test";
import { createVaultStatusRefresh, vaultInventoryFailureMessage } from "./vaultStatusRefresh";

test("an older mount list cannot restore a drive removed in a later observation", async () => {
  const observed: Array<string[] | null> = [];
  const refresh = createVaultStatusRefresh<string[]>(value => observed.push(value));
  let release!: (value: string[]) => void;
  const older = refresh(() => new Promise(resolve => { release = resolve; }));
  await refresh(async () => []);
  release(["J:"]);
  expect(await older).toBe(null);
  expect(observed).toEqual([[]]);
});

test("failed reads preserve the last observation but mark it stale and return no verification", async () => {
  const observed: Array<string[] | null> = [];
  const errors: unknown[] = [];
  const refresh = createVaultStatusRefresh<string[]>(value => observed.push(value), () => {}, error => errors.push(error));
  await refresh(async () => ["J:"]);
  expect(await refresh(async () => { throw new Error("service unavailable"); })).toBe(null);
  expect(observed).toEqual([["J:"]]);
  expect(errors[1] instanceof Error).toBe(true);
  await refresh(async () => []);
  expect(observed).toEqual([["J:"], []]);
  expect(errors[2]).toBe(null);
});

test("manual verification bypasses a timer request and later ticks join it instead of superseding", async () => {
  const values: Array<string[] | null> = [];
  const foreground: boolean[] = [];
  const refresh = createVaultStatusRefresh<string[]>(value => values.push(value), () => {}, () => {}, busy => foreground.push(busy));
  let finishTimer!: (value: string[]) => void;
  let finishManual!: (value: string[]) => void;
  let calls = 0;
  const timer = refresh(() => { ++calls; return new Promise(resolve => { finishTimer = resolve; }); }, true);
  await Promise.resolve();
  const manual = refresh(() => { ++calls; return new Promise(resolve => { finishManual = resolve; }); });
  const joined = refresh(async () => { ++calls; return ["wrong"]; }, true);
  await Promise.resolve();
  expect(calls).toBe(2);
  expect(foreground).toEqual([true]);
  finishManual([]);
  expect(await manual).toEqual([]);
  expect(await joined).toEqual([]);
  finishTimer(["old"]);
  await timer;
  expect(values).toEqual([[]]);
  expect(foreground).toEqual([true, false]);
});

test("an account-mode reset discards retained rows and outstanding old observations", async () => {
  const values: Array<string[] | null> = [];
  const refresh = createVaultStatusRefresh<string[]>(value => values.push(value));
  await refresh(async () => ["old account"]);
  let finish!: (value: string[]) => void;
  const old = refresh(() => new Promise(resolve => { finish = resolve; }));
  await Promise.resolve();
  refresh.reset();
  finish(["old response"]);
  await old;
  expect(values).toEqual([["old account"], null]);
});

test("synchronous probe failure cannot permanently coalesce later timer reads", async () => {
  const refresh = createVaultStatusRefresh<string[]>(() => {});
  expect(await refresh(() => { throw Error("failed"); }, true)).toBe(null);
  expect(await refresh(async () => ["fresh"], true)).toEqual(["fresh"]);
});

test("status failures are truthful, sanitized and never claim a successful dismount", () => {
  expect(vaultInventoryFailureMessage("caller_root_unavailable C:\\secret")).toContain("does not prove");
  expect(vaultInventoryFailureMessage("unknown C:\\secret")).toContain("last confirmed check");
  expect(vaultInventoryFailureMessage("unknown C:\\secret")).not.toContain("secret");
  expect(vaultInventoryFailureMessage("vault_service_personal_status_invalid")).toContain("Update or repair them together");
});

test("unverified mount identity is not reduced to a generic refresh or permission error", () => {
  const message = vaultInventoryFailureMessage(new Error("vault_mount_state_unknown C:\\private\\container"));
  expect(message).toContain("mount identity");
  expect(message).toContain("original mounting tool");
  expect(message).toContain("last confirmed check");
  expect(message).not.toContain("private");
  expect(message).not.toContain("Windows permissions");
});

test("an older failure cannot erase a newer confirmed observation", async () => {
  const observed: Array<string[] | null> = [];
  const refresh = createVaultStatusRefresh<string[]>(value => observed.push(value));
  let fail!: (reason: Error) => void;
  const older = refresh(() => new Promise((_, reject) => { fail = reject; }));
  await refresh(async () => ["K:"]);
  fail(new Error("old request failed"));
  await older;
  expect(observed).toEqual([["K:"]]);
});

test("refresh stays busy until every outstanding read settles, including failure", async () => {
  const busy: boolean[] = [];
  const refresh = createVaultStatusRefresh<string[]>(() => {}, value => busy.push(value));
  let release!: (value: string[]) => void;
  const older = refresh(() => new Promise(resolve => { release = resolve; }));
  await refresh(async () => { throw new Error("read failed"); });
  expect(busy).toEqual([true]);
  release([]);
  await older;
  expect(busy).toEqual([true, false]);
  await refresh(async () => []);
  expect(busy).toEqual([true, false, true, false]);
});
