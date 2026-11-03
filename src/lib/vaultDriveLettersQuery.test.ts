import { describe, expect, test } from "bun:test";
import { focusManager, QueryClient, QueryObserver } from "@tanstack/react-query";
import { refreshVaultDriveLetters, vaultDriveLettersQueryOptions } from "./vaultDriveLettersQuery";

describe("Vault drive availability refresh", () => {
  test("focus refreshes the single owner even while cached data is fresh", async () => {
    const client = new QueryClient();
    client.mount();
    focusManager.setFocused(false);
    let reads = 0;
    let current = ["J"];
    let published!: () => void;
    const onPublished = new Promise<void>((resolve) => { published = resolve; });
    const read = async () => {
      reads++;
      return current;
    };
    const owner = new QueryObserver(client, vaultDriveLettersQueryOptions("real", true, read));
    const passive = new QueryObserver(client, vaultDriveLettersQueryOptions("real", false, read));
    const stopOwner = owner.subscribe(() => {});
    const stopPassive = passive.subscribe(result => {
      if (result.data?.[0] === "K") published();
    });
    try {
      await owner.refetch();
      expect(reads).toBe(1);
      expect(passive.getCurrentResult().data).toEqual(["J"]);
      current = ["K"];
      focusManager.setFocused(true);
      await onPublished;
      expect(reads).toBe(2);
      expect(passive.getCurrentResult().data).toEqual(["K"]);
    } finally {
      stopPassive();
      stopOwner();
      client.unmount();
      client.clear();
      focusManager.setFocused(undefined);
    }
  });

  test("focus does not start a reader in decoy mode", async () => {
    const client = new QueryClient();
    client.mount();
    focusManager.setFocused(false);
    let reads = 0;
    const observer = new QueryObserver(client, vaultDriveLettersQueryOptions("decoy", true, async () => {
      reads++;
      return ["J"];
    }));
    const stop = observer.subscribe(() => {});
    try {
      focusManager.setFocused(true);
      await Promise.resolve();
      expect(reads).toBe(0);
    } finally {
      stop();
      client.unmount();
      client.clear();
      focusManager.setFocused(undefined);
    }
  });

  test("explicit refresh discards an older pending read before fetching fresh availability", async () => {
    const client = new QueryClient();
    let finishOld!: (value: string[]) => void;
    const oldRead = new Promise<string[]>((resolve) => { finishOld = resolve; });
    let reads = 0;
    const observer = new QueryObserver(client, vaultDriveLettersQueryOptions("real", true, async () => {
      reads++;
      return reads === 1 ? oldRead : ["K"];
    }));
    const stop = observer.subscribe(() => {});
    try {
      expect(reads).toBe(1);
      const fresh = await refreshVaultDriveLetters(client, "real", () => observer.refetch({ cancelRefetch: true }));
      expect(fresh.data).toEqual(["K"]);
      expect(reads).toBe(2);
      finishOld(["J"]);
      await Promise.resolve();
      expect(observer.getCurrentResult().data).toEqual(["K"]);
    } finally {
      stop();
      client.clear();
    }
  });
});
