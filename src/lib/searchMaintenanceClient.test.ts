import { describe, expect, test } from "bun:test";
import {
  createSearchMaintenanceClient,
  type SearchMaintenanceExecutor,
} from "./searchMaintenanceClient";

type Call = { command: string; args: Record<string, string | number | boolean> | undefined };

function makeClient() {
  const calls: Call[] = [];
  const execute: SearchMaintenanceExecutor = async (command, args) => {
    calls.push({ command, args });
    return { success: true, data: undefined };
  };
  return { calls, client: createSearchMaintenanceClient(execute) };
}

describe("Search maintenance command client", () => {
  test("keeps read-only viewers on their existing backend commands", async () => {
    const { calls, client } = makeClient();
    await client.getSearchIndexInfo();
    await client.getExplorerSearchHistoryInfo();
    await client.getSearchPersonalizationInfo();

    expect(calls).toEqual([
      { command: "Get-SearchIndexInfo", args: undefined },
      { command: "Get-ExplorerSearchHistoryInfo", args: undefined },
      { command: "Get-SearchPersonalizationInfo", args: undefined },
    ]);
  });

  test("keeps destructive actions routed through the established Clear command family", async () => {
    const { calls, client } = makeClient();
    await client.clearSearchIndex();
    await client.clearExplorerSearchHistory();
    await client.clearSearchPersonalizationData();

    expect(calls).toEqual([
      { command: "Clear-SearchIndex", args: undefined },
      { command: "Clear-ExplorerSearchHistory", args: undefined },
      { command: "Clear-SearchPersonalizationData", args: undefined },
    ]);
  });

  test("returns the central executor response unchanged", async () => {
    const response = { success: false, error: "backend unavailable" };
    const execute: SearchMaintenanceExecutor = async () => response;
    await expect(createSearchMaintenanceClient(execute).getSearchIndexInfo()).resolves.toBe(response);
  });
});
