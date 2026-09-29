import { expect, test } from "bun:test";
import { createVaultStatusRefresh } from "./vaultStatusRefresh";

test("an older mount list cannot restore a drive removed in a later observation", async () => {
  const observed: Array<string[] | null> = [];
  const refresh = createVaultStatusRefresh<string[]>(value => observed.push(value));
  let release!: (value: string[]) => void;
  const older = refresh(() => new Promise(resolve => { release = resolve; }));
  await refresh(async () => []);
  release(["J:"]);
  await older;
  expect(observed).toEqual([[]]);
});

test("failed current reads clear the stale snapshot rather than inventing an empty inventory", async () => {
  const observed: Array<string[] | null> = [];
  const refresh = createVaultStatusRefresh<string[]>(value => observed.push(value));
  await refresh(async () => ["J:"]);
  expect(await refresh(async () => { throw new Error("service unavailable"); })).toBe(null);
  expect(observed).toEqual([["J:"], null]);
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
