import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { FlowSettingsRecovery } from "./FlowSettingsRecovery";

test("flow recovery explains the restriction without trapping the rest of the app", () => {
  const html = renderToStaticMarkup(<FlowSettingsRecovery recoveryRequired />);
  expect(html).toContain('role="status"');
  expect(html).toContain("Your original files are preserved");
  expect(html).toContain("Automation stays locked");
  expect(html).toContain("continue using the rest of WinCommander");
  expect(html).not.toContain('role="dialog"');
  expect(html).not.toContain("<button");
});

test("temporary settings unavailability pauses automation without claiming encryption loss", () => {
  const html = renderToStaticMarkup(<FlowSettingsRecovery recoveryRequired={false} />);
  expect(html).toContain("Automation is temporarily unavailable");
  expect(html).toContain("Automation stays paused until access is restored");
  expect(html).toContain("continue using the rest of WinCommander");
  expect(html).not.toContain("cannot unlock");
  expect(html).not.toContain("needs personal data recovery");
  expect(html).not.toContain("original files");
  expect(html).not.toContain("<button");
});
