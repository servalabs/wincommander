import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Maintenance multi-row control accessibility", () => {
  test("names every record action and selector with its row", async () => {
    const [registry, startup, users, tasks, services, runtimes, malware, diskAnalyzer] = await Promise.all([
      Bun.file("src/panels/maintenance/RegistryTools.tsx").text(),
      Bun.file("src/components/tweaks/managers/StartupManager.tsx").text(),
      Bun.file("src/components/tweaks/managers/LocalUsersManager.tsx").text(),
      Bun.file("src/components/tweaks/managers/ScheduledTasksManager.tsx").text(),
      Bun.file("src/components/tweaks/managers/ServiceManager.tsx").text(),
      Bun.file("src/panels/runtime-visibility/index.tsx").text(),
      Bun.file("src/panels/maintenance/MalwareCenter.tsx").text(),
      Bun.file("src/panels/maintenance/DiskSpaceAnalyzerDialog.tsx").text(),
    ]);

    expect(registry).toContain("ariaLabel={`Select ${title}: ${currentPath}`}");
    expect(registry).toContain("aria-label={`${title}: ${currentPath}`}");
    expect(registry).toContain("aria-label={`Copy ${label}`}");
    expect(registry).toContain("aria-label={`Open parent folder for ${label}`}");

    expect(startup).toContain('aria-label="Search startup items"');
    expect(startup).toContain("aria-label={`Open folder containing ${item.Name}`}");
    expect(startup).toContain('title={recTip}');
    expect(startup).not.toContain("<Tooltip content={recTip}>");
    expect(startup).toContain('aria-label={`${item.IsEnabled ? "Disable" : "Enable"} ${item.Name} at startup`}');

    expect(users).toContain('aria-label="Search local users"');
    expect(users).toContain("aria-label={`Hide ${user.name} from login`}");

    expect(tasks).toContain("aria-label={`Run ${t.Name}`}");
    expect(tasks).toContain('aria-label={`${disabled ? "Enable" : "Disable"} ${t.Name}`}');
    expect(tasks).toContain("aria-label={`Delete ${t.Name}`}");

    expect(services).toContain("aria-label={`Startup mode for ${svc.DisplayName}`}");
    expect(services).toContain('aria-label={`${svc.State === "Running" ? "Stop" : "Start"} ${svc.DisplayName}`}');
    expect(services).toContain("aria-pressed={showOnlyRecommended}");

    expect(runtimes).toContain('title={KIND_DETAIL[r.kind]}');
    expect(runtimes).toContain("aria-label={`Open folder containing ${r.name}`}");
    expect(runtimes).toContain("aria-label={`Restore ${r.name}`}");
    expect(runtimes).toContain("aria-label={`Hide ${r.name}`}");

    expect(malware).toContain("aria-label={`Restore ${entry.threatLabel}`}");
    expect(malware).toContain("aria-label={`Delete ${entry.threatLabel}`}");
    expect(malware).toContain("aria-label={`Quarantine ${finding.threatLabel}`}");

    expect(diskAnalyzer).toContain('aria-pressed={viewMode === "space"}');
    expect(diskAnalyzer).toContain('aria-pressed={spaceViewStyle === "treemap"}');
    expect(diskAnalyzer).toContain("aria-pressed={largeFilter === id}");
    expect(diskAnalyzer).toContain('aria-label={`Open folder containing ${row.name}`}');
    expect(diskAnalyzer).toContain('aria-label={`Delete ${row.name}`}');
    expect(diskAnalyzer).toContain('<th className="da-th-del">Actions</th>');
  });
});
