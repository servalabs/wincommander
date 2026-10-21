import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Maintenance multi-row control accessibility", () => {
  test("names every record action and selector with its row", async () => {
    const [registry, startup, users, tasks, services, runtimes, malware, diskAnalyzer] = await Promise.all([
      Bun.file("src/panels/maintenance/RegistryTools.tsx").text(),
      Promise.all(["StartupManager.tsx", "StartupReviewRow.tsx"].map(path => Bun.file(`src/components/tweaks/managers/${path}`).text())).then(parts => parts.join("\n")),
      Bun.file("src/components/tweaks/managers/LocalUsersManager.tsx").text(),
      Bun.file("src/components/tweaks/managers/ScheduledTasksManager.tsx").text(),
      Bun.file("src/components/tweaks/managers/ServiceManager.tsx").text(),
      Bun.file("src/panels/runtime-visibility/index.tsx").text(),
      Bun.file("src/panels/maintenance/MalwareCenter.tsx").text(),
      Promise.all(["DiskSpaceAnalyzerDialog.tsx", "disk-analyzer/DiskAnalyzerHeader.tsx", "disk-analyzer/DiskItemList.tsx", "disk-analyzer/DiskItemDetails.tsx"].map(path => Bun.file(`src/panels/maintenance/${path}`).text())).then(parts => parts.join("\n")),
    ]);

    expect(registry).toContain("ariaLabel={`Select ${title}: ${currentPath}`}");
    expect(registry).toContain("aria-label={`${title}: ${currentPath}`}");
    expect(registry).toContain("aria-label={`Copy ${label}`}");
    expect(registry).toContain("aria-label={`Open parent folder for ${label}`}");

    expect(startup).toContain('aria-label="Search startup items"');
    expect(startup).toContain('aria-controls={detailId}');
    expect(startup).toContain('Startup location');
    expect(startup).toContain('Startup delay is not measured.');
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

    expect(diskAnalyzer).not.toContain('aria-label="Storage view"');
    expect(diskAnalyzer).toContain('aria-label="Minimum file size"');
    expect(diskAnalyzer).toContain("Open in Explorer");
    expect(diskAnalyzer).toContain('aria-pressed={chartMode === "treemap"}');
    expect(diskAnalyzer).toContain("aria-pressed={filter === item.id}");
    expect(diskAnalyzer).toContain('aria-label={`Open ${item.name}`}');
    expect(diskAnalyzer).toContain('aria-label="Selected item details"');
    expect(diskAnalyzer).toContain('ariaLabel={`Select ${item.name}');
  });

  test("keeps maintenance-specific loading, error, and detail contracts intact", async () => {
    const [fileStats, hygiene, fileHygiene, routinePreview, startupDrivers, security, malware, registry, diskAnalyzer] = await Promise.all([
      Bun.file("src/panels/maintenance/FileStatsPanel.tsx").text(),
      Bun.file("src/panels/maintenance/SystemHygieneTools.tsx").text(),
      Bun.file("src/panels/maintenance/FileHygieneTools.tsx").text(),
      Bun.file("src/panels/maintenance/RoutineCleanerPreview.tsx").text(),
      Bun.file("src/panels/maintenance/StartupDriverTools.tsx").text(),
      Bun.file("src/panels/maintenance/SecurityData.tsx").text(),
      Bun.file("src/panels/maintenance/MalwareCenter.tsx").text(),
      Bun.file("src/panels/maintenance/RegistryTools.tsx").text(),
      Promise.all(["DiskSpaceAnalyzerDialog.tsx", "disk-analyzer/DiskAnalyzerHeader.tsx", "disk-analyzer/DiskItemList.tsx", "disk-analyzer/DiskItemDetails.tsx"].map(path => Bun.file(`src/panels/maintenance/${path}`).text())).then(parts => parts.join("\n")),
    ]);

    expect(fileStats).toContain('role="table" aria-label="File type statistics"');
    expect(fileStats).toContain('role="columnheader">File type</span>');
    expect(fileStats).toContain('role="alert"');
    expect(fileStats).toContain('!fileStats && !statsScanning && !scanError');

    expect(hygiene).toContain("<CheckboxControl checked={checked} onChange={onClick}");
    expect(hygiene).toContain('ariaLabel={`${checked ? "Deselect" : "Select"} ${title}`}');
    expect(hygiene).toContain('Reviewing {tools.tool === "shortcuts"');
    expect(hygiene).toContain('xl:grid-cols-2');
    expect(fileHygiene).toContain('ariaLabel={`${checked ? "Deselect" : "Select"} ${label}`}');
    expect(routinePreview).toContain('ariaLabel={`${selected ? "Deselect" : "Select"} ${item.label}`}');
    expect(routinePreview).not.toContain('onClick={() => !disabled && onToggle(item.id)}');

    expect(startupDrivers).toContain('aria-controls={`maintenance-manager-panel-${id}`}');
    expect(startupDrivers).toContain('tabIndex={active ? 0 : -1}');
    expect(startupDrivers).toContain("const moveManagerTab = (key: string)");
    expect(startupDrivers).toContain('role="tabpanel"');
    expect(security).toContain('Collecting local security posture and Windows CVE coverage');
    expect(security).toContain('aria-busy={loading}');
    expect(malware).toContain('Loading quarantine inventory');
    expect(malware).toContain('role="progressbar"');
    expect(malware).toContain('xl:grid-cols-2');
    expect(registry).toContain('Reviewing {tools.tool === "orphans"');

    expect(diskAnalyzer).toContain('aria-label="Drive or folder to analyze"');
    expect(diskAnalyzer).toContain('aria-label="Selected item details"');
    expect(diskAnalyzer).toContain('Sizes are logical file sizes');
    expect(diskAnalyzer).toContain('className="da-scan-notice" role="status"');
  });
});
