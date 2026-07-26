import { describe, expect, test } from "bun:test";
import { alertsBadge, processesBadge, splitNotificationsByKind } from "./badgeCount";
import { notifKind, type AppNotification } from "./notificationStore";

const notif = (
  severity: AppNotification["severity"],
  message: string,
  kind?: AppNotification["kind"],
): AppNotification => ({
  id: `${severity}-${message}`,
  severity,
  message,
  kind,
  time: "2026-07-26T00:00:00.000Z",
});

describe("notifKind", () => {
  test("info severity defaults to notification (operational)", () => {
    expect(notifKind({ severity: "info", kind: undefined })).toBe("notification");
  });

  test("warn severity defaults to alert (security)", () => {
    expect(notifKind({ severity: "warn", kind: undefined })).toBe("alert");
  });

  test("danger severity defaults to alert (security)", () => {
    expect(notifKind({ severity: "danger", kind: undefined })).toBe("alert");
  });

  test("explicit kind overrides the severity default", () => {
    // A danger-severity error explicitly tagged "notification" (the pattern
    // used throughout the app, e.g. RightSidebar's mount-failure toasts) must
    // route as operational, not as a security alert.
    expect(notifKind({ severity: "danger", kind: "notification" })).toBe("notification");
    expect(notifKind({ severity: "info", kind: "alert" })).toBe("alert");
  });
});

describe("splitNotificationsByKind", () => {
  test("routes by notifKind(), not by raw severity", () => {
    const notifs = [
      notif("danger", "wrong password"),                          // -> alert (default)
      notif("danger", "mount failed", "notification"),             // -> explicit override
      notif("info", "update available"),                           // -> notification (default)
      notif("warn", "driver outdated"),                             // -> alert (default)
    ];
    const { alertNotifs, opsNotifs } = splitNotificationsByKind(notifs);
    expect(alertNotifs.map((n) => n.message)).toEqual(["wrong password", "driver outdated"]);
    expect(opsNotifs.map((n) => n.message)).toEqual(["mount failed", "update available"]);
  });

  test("empty input produces empty sections", () => {
    const { alertNotifs, opsNotifs } = splitNotificationsByKind([]);
    expect(alertNotifs).toEqual([]);
    expect(opsNotifs).toEqual([]);
  });
});

describe("alertsBadge", () => {
  test("counts only what it's given — operational items must be pre-filtered out by the caller", () => {
    const alertNotifs = [notif("warn", "a"), notif("danger", "b")];
    expect(alertsBadge(alertNotifs).count).toBe(2);
  });

  test("danger beats warn for badge colour", () => {
    const mixed = [notif("warn", "a"), notif("danger", "b")];
    expect(alertsBadge(mixed).color).toBe("var(--danger)");
  });

  test("warn-only list gets the warn colour", () => {
    const warnOnly = [notif("warn", "a"), notif("warn", "b")];
    expect(alertsBadge(warnOnly).color).toBe("var(--warn)");
  });

  test("empty list counts zero", () => {
    expect(alertsBadge([]).count).toBe(0);
  });
});

describe("processesBadge", () => {
  test("counts running tasks only — completed/failed tasks don't inflate it", () => {
    const tasks = [
      { status: "running" as const },
      { status: "running" as const },
      { status: "completed" as const },
      { status: "failed" as const },
    ];
    expect(processesBadge(tasks).count).toBe(2);
  });

  test("zero running tasks gives a zero count even with finished tasks still listed", () => {
    const tasks = [{ status: "completed" as const }, { status: "failed" as const }];
    expect(processesBadge(tasks).count).toBe(0);
  });

  test("any failed task turns the badge danger-coloured, even while others are running", () => {
    const tasks = [{ status: "running" as const }, { status: "failed" as const }];
    expect(processesBadge(tasks).color).toBe("var(--danger)");
  });

  test("no failures gives the accent colour", () => {
    const tasks = [{ status: "running" as const }, { status: "completed" as const }];
    expect(processesBadge(tasks).color).toBe("var(--accent)");
  });
});
