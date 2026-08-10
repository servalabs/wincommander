import { describe, expect, it } from "bun:test";

// The store is deliberately a browser-side localStorage journal. This small
// in-memory implementation makes its persistence contract testable in Bun.
const values = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
  },
});

const store = await import("./packageActivityStore");

describe("package activity journal", () => {
  it("retains terminal package work and exposes queued work", () => {
    const id = store.recordPackageActivity({ packageId: "Acme.Tool", label: "Acme Tool", kind: "install" });
    expect(store.getPackageActivities()[0]).toMatchObject({ id, status: "queued", packageId: "Acme.Tool" });

    store.setPackageActivityStatus(id, "running");
    store.setPackageActivityStatus(id, "completed");
    expect(store.getPackageActivities()[0]).toMatchObject({ id, status: "completed" });
    expect(values.get("wc-package-activity-v1")).toContain("Acme.Tool");
  });

  it("clears only terminal history", () => {
    const active = store.recordPackageActivity({ packageId: "Active.Tool", label: "Active Tool", kind: "update" });
    store.clearFinishedPackageActivities();
    expect(store.getPackageActivities()).toHaveLength(1);
    expect(store.getPackageActivities()[0]).toMatchObject({ id: active, status: "queued" });
  });
});
