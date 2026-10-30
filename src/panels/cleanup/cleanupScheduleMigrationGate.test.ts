import { expect, test } from "bun:test";
import { createCleanupScheduleMigrationGate, isVerifiedCleanupMigration } from "./cleanupScheduleMigrationGate";

test("only an explicit complete migration receipt is accepted", () => {
  expect(isVerifiedCleanupMigration({ success: true, data: { status: "ok", migrated: [] } })).toBe(true);
  expect(isVerifiedCleanupMigration({ success: true })).toBe(false);
  expect(isVerifiedCleanupMigration({ success: true, data: { status: "partial", migrated: [] } })).toBe(false);
  expect(isVerifiedCleanupMigration({ success: true, data: { status: "ok", migrated: [], error: true } })).toBe(false);
  expect(isVerifiedCleanupMigration({ success: true, data: { status: "ok" } })).toBe(false);
});

test("failed and throwing migrations remain retryable, then successful migration is reused", async () => {
  const gate = createCleanupScheduleMigrationGate();
  expect(await gate.ensure(async () => false)).toBe(false);
  expect(await gate.ensure(async () => { throw new Error("failure"); })).toBe(false);
  expect(await gate.ensure(() => { throw new Error("synchronous failure"); })).toBe(false);
  let calls = 0;
  expect(await gate.ensure(async () => { calls++; return true; })).toBe(true);
  expect(await gate.ensure(async () => { calls++; return false; })).toBe(true);
  expect(calls).toBe(1);
});

test("concurrent schedule reads share one pending migration", async () => {
  const gate = createCleanupScheduleMigrationGate();
  let finish!: (value: boolean) => void;
  let calls = 0;
  const migrate = () => { calls++; return new Promise<boolean>(resolve => { finish = resolve; }); };
  const first = gate.ensure(migrate);
  const second = gate.ensure(migrate);
  await Promise.resolve();
  expect(calls).toBe(1);
  finish(true);
  expect(await first).toBe(true);
  expect(await second).toBe(true);
});
