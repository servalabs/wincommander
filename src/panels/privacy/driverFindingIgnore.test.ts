import { describe, expect, test } from "bun:test";
import {
  driverHealthIgnoreId,
  ignoredDriverFindingCount,
  isIgnoredDriverFinding,
  vulnerableDriverIgnoreId,
} from "./driverFindingIgnore";

describe("driver finding ignores", () => {
  test("uses normalized, namespaced identifiers that cannot collide with radar findings", () => {
    expect(driverHealthIgnoreId({ instanceId: " USB\\VID_123 ", name: "ignored", problemCode: 43 }))
      .toBe("driver-health:usb\\vid_123");
    expect(vulnerableDriverIgnoreId({ filename: "legacy.sys", path: " C:\\Windows\\System32\\drivers\\legacy.sys " }))
      .toBe("driver-byovd:c:\\windows\\system32\\drivers\\legacy.sys");
  });

  test("counts only driver dismissals and keeps unrelated dashboard dismissals intact", () => {
    const ignored = ["network:dns", "driver-health:usb\\x", "driver-byovd:c:\\legacy.sys"];
    expect(ignoredDriverFindingCount(ignored)).toBe(2);
    expect(isIgnoredDriverFinding(ignored, "driver-health:usb\\x")).toBe(true);
    expect(isIgnoredDriverFinding(ignored, "network:dns")).toBe(true);
  });
});
