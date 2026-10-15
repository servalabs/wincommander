import { describe, expect, test } from "bun:test";
import { QueryClient, QueryObserver } from "@tanstack/react-query";
import type { AppSettings } from "../types/settings";
import { createSettingsCacheObserver, settingsKeys, settingsQueryOptions } from "./settingsQueryPolicy";

const privateSettings = { app: { advisor: { endpoint: "private-fixture" } } } as unknown as AppSettings;
const updatedSettings = { app: { advisor: { endpoint: "updated-private-fixture" } } } as unknown as AppSettings;

describe("settings query mode isolation", () => {
  test("a cache-only subscriber receives writes without fetching or disabling the owner reader", async () => {
    const client = new QueryClient();
    let reads = 0;
    const read = async () => { reads += 1; return updatedSettings; };
    client.setQueryData(settingsKeys.detail(), privateSettings);
    const passive = createSettingsCacheObserver(client, "real");
    const unsubscribePassive = passive.subscribe(() => {});
    expect(reads).toBe(0);
    client.setQueryData(settingsKeys.detail(), updatedSettings);
    expect(passive.getCurrentResult().data).toEqual(updatedSettings);
    expect(reads).toBe(0);
    const owner = new QueryObserver(client, { ...settingsQueryOptions("real", read), staleTime: Infinity });
    const unsubscribeOwner = owner.subscribe(() => {});
    const laterPassive = createSettingsCacheObserver(client, "real");
    const unsubscribeLater = laterPassive.subscribe(() => {});
    await client.invalidateQueries({ queryKey: settingsKeys.detail() });
    expect(reads).toBe(1);
    expect(passive.getCurrentResult().data).toEqual(updatedSettings);
    const decoy = createSettingsCacheObserver(client, "decoy");
    const unsubscribeDecoy = decoy.subscribe(() => {});
    client.setQueryData(settingsKeys.decoy(), privateSettings);
    expect(reads).toBe(1);
    expect(decoy.getCurrentResult().data).toBeUndefined();
    unsubscribeDecoy();
    unsubscribeLater();
    unsubscribePassive();
    unsubscribeOwner();
    client.clear();
  });

  test("hides an existing real cache without replacing the shared QueryClient", () => {
    const client = new QueryClient();
    client.setQueryData(settingsKeys.detail(), privateSettings);
    const observer = new QueryObserver(client, settingsQueryOptions("real", async () => privateSettings));
    expect(observer.getCurrentResult().data).toEqual(privateSettings);
    observer.setOptions(settingsQueryOptions("decoy", async () => privateSettings));
    expect(observer.getCurrentResult().data).toBeUndefined();
    expect(client.getQueryData(settingsKeys.detail())).toEqual(privateSettings);
    client.clear();
  });

  test("late real reads and mutation publications cannot populate the decoy view", async () => {
    const client = new QueryClient();
    let complete!: (settings: AppSettings) => void;
    const pending = new Promise<AppSettings>((resolve) => { complete = resolve; });
    const observer = new QueryObserver(client, settingsQueryOptions("real", () => pending));
    const unsubscribe = observer.subscribe(() => {});
    const read = observer.refetch();
    observer.setOptions(settingsQueryOptions("decoy", () => pending));
    complete(privateSettings);
    await read;
    client.setQueryData(settingsKeys.detail(), updatedSettings);
    expect(observer.getCurrentResult().data).toBeUndefined();
    expect(client.getQueryData(settingsKeys.detail())).toEqual(updatedSettings);
    unsubscribe();
    client.clear();
  });

  test("an explicit decoy refetch never calls the real backend reader", async () => {
    const client = new QueryClient();
    let reads = 0;
    const observer = new QueryObserver(client, settingsQueryOptions("decoy", async () => {
      reads += 1;
      return privateSettings;
    }));
    const result = await observer.refetch();
    expect(reads).toBe(0);
    expect(result.data).toBeUndefined();
    client.clear();
  });

  test("returning to real mode retains normal publication and invalidation", async () => {
    const client = new QueryClient();
    client.setQueryData(settingsKeys.detail(), privateSettings);
    const observer = new QueryObserver(client, settingsQueryOptions("decoy", async () => updatedSettings));
    const unsubscribe = observer.subscribe(() => {});
    observer.setOptions(settingsQueryOptions("real", async () => updatedSettings));
    await observer.refetch();
    expect(observer.getCurrentResult().data).toEqual(updatedSettings);
    expect(client.getQueryData(settingsKeys.detail())).toEqual(updatedSettings);
    await client.invalidateQueries({ queryKey: settingsKeys.all });
    expect(observer.getCurrentResult().data).toEqual(updatedSettings);
    unsubscribe();
    client.clear();
  });
});
