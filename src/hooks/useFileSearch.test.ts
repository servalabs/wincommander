// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, test } from "bun:test";
import { cancelPendingFileSearch } from "./useFileSearch";

describe("filename search cancellation", () => {
  test("clear and mount changes cancel queued queries and invalidate late responses", async () => {
    let searches = 0;
    const timer = { current: setTimeout(() => { searches += 1; }, 1) as ReturnType<typeof setTimeout> | null };
    const request = { current: 1 };
    const responseRequest = request.current;
    cancelPendingFileSearch(timer, request);
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(searches).toBe(0);
    expect(timer.current).toBeNull();
    expect(request.current).not.toBe(responseRequest);
  });

});
