import { expect, test } from "bun:test";

const source = await Bun.file("src/panels/privacy/UsbDevicesSection.tsx").text();

test("USB mutation failures persist bounded diagnostics without device data", () => {
  for (const [action, code] of [["block", "USB.BLOCK.FAILED"], ["allow", "USB.ALLOW.FAILED"]]) {
    expect(source).toContain(`recordUsbFailure('${action}', '${code}');`);
  }
  const helper = source.slice(source.indexOf("const recordUsbFailure ="), source.indexOf("const refreshVolumes ="));
  expect(helper).toContain("recordDiagnostic({");
  expect(helper).not.toMatch(/friendlyName|instanceId|deviceKey|reason|message:/);
});

test("HID approval rejection is distinguished from a Windows mutation attempt", () => {
  expect(source).toContain("recordUsbFailure('allow', 'USB.HID_APPROVAL.REQUIRED', false)");
  expect(source).toContain("stage: attempted ? 'windows_apply' : 'validation'");
  expect(source).toContain("lifecycle: attempted ? 'applied' : 'acknowledged'");
});
