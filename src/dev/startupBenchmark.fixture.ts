export function completeStartupFixture() {
  return {
    schemaVersion: 1,
    metadata: {
      freeRevision: "a".repeat(40),
      proRevision: "b".repeat(40),
      freeArtifactHash: "c".repeat(64),
      proArtifactHash: "d".repeat(64),
      machineId: "synthetic-test-machine",
      windowsVersion: "synthetic-windows-version",
      webviewVersion: "synthetic-webview-version",
      capturedAt: "2026-09-22T00:00:00.000Z",
      downloadsEntries: 50_000,
      protectionRequired: true,
    },
    samples: [{
      scenario: "warm",
      elapsedMs: {
        process_start: 0,
        native_setup_entered: 10,
        main_window_show_requested: 20,
        webview_dom_ready: 30,
        settings_cache_hydrated: 40,
        dashboard_first_visible: 50,
        dashboard_interactive: 60,
        protection_required_ready: 55,
        fresh_system_probe_complete: 70,
        background_idle: 80,
      },
    }],
  };
}
