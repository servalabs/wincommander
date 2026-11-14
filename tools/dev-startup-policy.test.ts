import { expect, test } from "bun:test";
import { developmentStartupPolicy as policy, vitePortOwner } from "./dev-startup-policy";

test("preserve mode fails closed when port inspection fails", () => {
  expect(() => vitePortOwner("", 1, true)).toThrow("existing processes were preserved");
  expect(vitePortOwner("", 1, false)).toBeNull();
  expect(vitePortOwner("  TCP  127.0.0.1:1420  0.0.0.0:0  LISTENING  1234\r\n", 0, true)).toBe("1234");
  expect(vitePortOwner("  TCP  127.0.0.1:11420  0.0.0.0:0  LISTENING  1234\r\n", 0, true)).toBeNull();
});

test("session-preserving Free startup never kills processes or restarts the shared service", () => {
  expect(policy({ preserveSessions: true, freeOnly: true, precleaned: false, portOwner: null }))
    .toEqual({ cleanProcesses: false, releasePort: false, noServiceRestart: true });
});

test("session-preserving startup refuses an occupied port and unsafe build modes", () => {
  const base = { preserveSessions: true, freeOnly: true, precleaned: false, portOwner: null };
  expect(() => policy({ ...base, portOwner: "1234" })).toThrow("listener was preserved");
  expect(() => policy({ ...base, freeOnly: false })).toThrow("requires --free");
  expect(() => policy({ ...base, precleaned: true })).toThrow("pre-cleaned");
});

test("ordinary startup keeps the existing stale-process and service-update behavior", () => {
  expect(policy({ preserveSessions: false, freeOnly: false, precleaned: false, portOwner: "1234" }))
    .toEqual({ cleanProcesses: true, releasePort: true, noServiceRestart: false });
  expect(policy({ preserveSessions: false, freeOnly: false, precleaned: true, portOwner: null }))
    .toEqual({ cleanProcesses: false, releasePort: true, noServiceRestart: false });
});
