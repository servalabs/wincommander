import { decoyEventKind, decoyEventLabel, decoyEventToast } from "./decoyEventPresentation";
import { describe, expect, test } from "bun:test";

describe("decoy event presentation", () => {
  test("labels the stored opened event as access, rather than a modification", () => {
    expect(decoyEventKind("opened")).toBe("opened");
    expect(decoyEventLabel("opened")).toBe("Opened or read");
    expect(decoyEventToast("opened", "budget.xlsx")).toContain("This is an access event");
  });

  test("keeps historical read events legible after the event-name migration", () => {
    expect(decoyEventKind("read")).toBe("opened");
    expect(decoyEventLabel("read")).toBe("Opened or read");
  });

  test("labels filesystem changes separately and never claims they prove an open", () => {
    for (const kind of ["modified", "renamed", "removed"]) {
      expect(decoyEventKind(kind)).not.toBe("read");
      expect(decoyEventToast(kind, "budget.xlsx")).toContain("not proof that someone opened it");
    }
    expect(decoyEventLabel("modified")).toBe("Changed");
  });

  test("uses a safe generic label for a future or malformed event kind", () => {
    expect(decoyEventLabel("metadata")).toBe("Activity detected");
  });
});
