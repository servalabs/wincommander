import { expect, test } from "bun:test";
import type { EncryptionStatus } from "@/hooks/useBackend";
import { canOpenQuickMountDrive } from "./quickMountObservation";

test("a cached Quick Mount receipt cannot open an absent, replaced or unverified drive", () => {
  const status = (volumes: unknown[]) => ({ volumes }) as EncryptionStatus;
  const own = { letter: "S:", path: "D:\\Vault", accessible: true };
  expect(canOpenQuickMountDrive(status([own]), null, "s", "D:\\Vault")).toBe(true);
  expect(canOpenQuickMountDrive(status([]), null, "S:", "D:\\Vault")).toBe(false);
  expect(canOpenQuickMountDrive(status([{ ...own, path: "D:\\Other" }]), null, "S:", "D:\\Vault")).toBe(false);
  expect(canOpenQuickMountDrive(status([{ ...own, accessible: false }]), null, "S:", "D:\\Vault")).toBe(false);
  expect(canOpenQuickMountDrive(status([own]), "unavailable", "S:", "D:\\Vault")).toBe(false);
  expect(canOpenQuickMountDrive(null, null, "S:", "D:\\Vault")).toBe(false);
});
