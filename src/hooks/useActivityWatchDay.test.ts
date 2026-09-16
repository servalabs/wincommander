import { describe, expect, it } from "bun:test";
import { activityWatchStartupMessage } from "./useActivityWatchDay";

describe("ActivityWatch startup status", () => {
  it("explains duplicate watcher protection without exposing process details", () => {
    expect(activityWatchStartupMessage("activitywatch_duplicate_window_watcher"))
      .toContain("No new tracker was started");
  });

  it("keeps an unknown supervisor failure bounded", () => {
    expect(activityWatchStartupMessage(new Error("C:\\private\\ActivityWatch.exe failed")))
      .toBe("ActivityWatch could not be started or reached. Check Productivity settings, then retry.");
  });
});
