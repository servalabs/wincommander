import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

async function read(path: string): Promise<string> {
  return (await Bun.file(path).text()).replace(/\r\n/g, "\n");
}

describe("USB Protection truthfulness and lifecycle contracts", () => {
  test("default card is concise and exposes the three primary areas", async () => {
    const source = await read("src/panels/privacy/UsbDevicesSection.tsx");
    expect(source).toContain('title="USB Protection"');
    expect(source).toContain("Devices currently connected");
    expect(source).toContain("Alerts today");
    expect(source).toContain("Last USB event");
    expect(source).toContain(">Timeline</h3>");
    expect(source).toContain(">Trusted devices</h3>");
    expect(source).toContain(">Protection actions</h3>");
    expect(source).toContain("does not inspect files or typed content");
  });

  test("timeline separates current, persisted, unknown, and unsupported history", async () => {
    const source = await read("src/panels/privacy/UsbDevicesSection.tsx");
    for (const label of [
      "Connected now",
      "Current monitor run",
      "Persisted monitor record",
      "Present when armed",
      "State unknown",
      "Unknown gaps stay unknown",
    ]) expect(source).toContain(label);
    expect(source).toContain("does not import complete Windows USB history");
    expect(source).toContain("cannot say what USB activity occurred while protection was off");
    expect(source).not.toContain("No USB device events recorded.");
    expect(source).toContain("isInternalUsbPlumbing");
    expect(source).toContain("root hub|host controller|generic usb hub");
  });

  test("advanced protection areas are collapsed disclosures", async () => {
    const source = await read("src/panels/privacy/UsbDevicesSection.tsx");
    for (const heading of [
      "Transfer monitoring",
      "Keyboard/HID safety",
      "Auto-isolate",
      "Advanced policy",
    ]) expect(source).toContain(`<summary className="cursor-pointer text-sm font-semibold">${heading}</summary>`);
  });

  test("one master arm state gates every child collector and never locally claims Windows verified a block", async () => {
    const source = await read("src/panels/privacy/UsbDevicesSection.tsx");
    const app = await read("src/App.tsx");
    expect(source).toContain("const protectionActive = masterEnabled && status.running;");
    expect(source).toContain("Advanced USB settings below are saved configuration, not running.");
    expect(source).toContain("if (!protectionActive) return;");
    expect(source).toContain("disabled={childControlsDisabled}");
    expect(source).toContain("Block requested; Windows verification is pending.");
    expect(source).not.toContain('now disabled in Windows.');
    expect(app).toContain("const paidMonitorDesired = usbMonitorEnabled && hasPaid && paidMonitorConfigured;");
    expect(app).toContain("const basicMonitorDesired = usbMonitorEnabled;");
  });

  test("info help works with mouse, keyboard, click, Escape, and touch-compatible click", async () => {
    const source = await read("src/panels/privacy/UsbDevicesSection.tsx");
    expect(source).toContain("onMouseEnter={() => setOpen(true)}");
    expect(source).toContain("onMouseLeave={() => { if (!pinned) setOpen(false); }}");
    expect(source).toContain("onFocus={() => setOpen(true)}");
    expect(source).toContain("onBlur={() => { if (!pinned) setOpen(false); }}");
    expect(source).toContain("onClick={() => {");
    expect(source).toContain("event.key === 'Escape'");
    expect(source).toContain("aria-expanded={open}");
    expect(source).toContain('role="tooltip"');
  });

  test("backend surfaces watcher health instead of converting source failure to empty history", async () => {
    const rust = await read("src-tauri/commander-free/src/usb_guard.rs");
    expect(rust).toContain("classify_snapshot_failure");
    expect(rust).toContain('"permission_denied"');
    expect(rust).toContain('"query_timeout"');
    expect(rust).toContain('"source_unavailable"');
    expect(rust).toContain('"persistence_failed"');
    expect(rust).toContain('"historyCoverage": "monitorOnly"');
    expect(rust).toContain('"windowsHistoryAvailable": false');
    expect(rust).not.toContain("if !output.status.success() {\n        return Ok(Vec::new());");
  });

  test("backend has focused lifecycle, readback, and safe-error tests", async () => {
    const rust = await read("src-tauri/commander-free/src/usb_guard.rs");
    expect(rust).toContain("lifecycle_arm_attach_refresh_detach_preserves_duration");
    expect(rust).toContain("persisted_open_session_is_not_assumed_current_after_restart");
    expect(rust).toContain("persisted_timeline_round_trip_keeps_monitor_evidence_without_sensitive_content");
    expect(rust).toContain("source_failures_map_to_safe_recovery_reasons");
    expect(rust).toContain("attached_at_estimated: first_poll");
    expect(rust).toContain("basic_current_keys().lock().unwrap().clear();");
  });
});
