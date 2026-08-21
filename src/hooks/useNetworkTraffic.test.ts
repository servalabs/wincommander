import { describe, expect, test } from "bun:test";
import {
  NETWORK_TRAFFIC_HISTORY,
  getNetworkTrafficSnapshot,
  recordNetworkTrafficSample,
  resetNetworkTrafficForTests,
} from "./useNetworkTraffic";

describe("network traffic store", () => {
  test("keeps the latest sample and rolling upload/download histories", () => {
    resetNetworkTrafficForTests();

    recordNetworkTrafficSample({ upBytesPerSec: 10, downBytesPerSec: 20 });
    expect(getNetworkTrafficSnapshot()).toEqual({
      sample: { upBytesPerSec: 10, downBytesPerSec: 20 },
      upHistory: [10],
      downHistory: [20],
    });

    for (let i = 0; i < NETWORK_TRAFFIC_HISTORY + 5; i += 1) {
      recordNetworkTrafficSample({ upBytesPerSec: i, downBytesPerSec: i * 2 });
    }

    const snapshot = getNetworkTrafficSnapshot();
    expect(snapshot.upHistory.length).toBe(NETWORK_TRAFFIC_HISTORY);
    expect(snapshot.downHistory.length).toBe(NETWORK_TRAFFIC_HISTORY);
    expect(snapshot.sample).toEqual({
      upBytesPerSec: NETWORK_TRAFFIC_HISTORY + 4,
      downBytesPerSec: (NETWORK_TRAFFIC_HISTORY + 4) * 2,
    });
  });
});
