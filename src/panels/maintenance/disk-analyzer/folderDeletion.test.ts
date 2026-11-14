import { expect, test } from "bun:test";
import { performFolderDeletion } from "./folderDeletion";

test("cancelled native confirmation never requests deletion", async () => {
  let invoked = false;
  const result = await performFolderDeletion({
    confirmDiskFolderDelete: async () => null,
    deleteConfirmedDiskFolder: async () => { invoked = true; throw new Error("must not run"); },
  }, "scan", 42);
  expect(result).toEqual({ kind: "cancelled", attempted: false });
  expect(invoked).toBe(false);
});

test("confirmed token is sent only with the same scanned item", async () => {
  const calls: unknown[] = [];
  const result = await performFolderDeletion({
    confirmDiskFolderDelete: async (scan, item) => { calls.push(["confirm", scan, item]); return "single-use"; },
    deleteConfirmedDiskFolder: async (scan, item, token) => { calls.push(["delete", scan, item, token]); return { status: "partial", processed: 1, cacheInvalidated: true }; },
  }, "scan-7", 42);
  expect(calls).toEqual([["confirm", "scan-7", 42], ["delete", "scan-7", 42, "single-use"]]);
  expect(result).toEqual({ kind: "receipt", attempted: true, receipt: { status: "partial", processed: 1, cacheInvalidated: true } });
});

test("transport errors after a deletion request remain potentially destructive", async () => {
  const result = await performFolderDeletion({
    confirmDiskFolderDelete: async () => "token",
    deleteConfirmedDiskFolder: async () => { throw new Error("connection lost"); },
  }, "scan", 42);
  expect(result.kind).toBe("error");
  expect(result.attempted).toBe(true);
});
