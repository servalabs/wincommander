import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { PersonalSettingsNotice, PersonalSettingsRecoveryDetails } from "./PersonalSettingsNotice";

test("recovery notice preserves access to the app and explains old data separately from new preferences", () => {
  const html = renderToStaticMarkup(<PersonalSettingsNotice status={{ mode: "service", recoveryRequired: true, canSave: true }} />);

  expect(html).toContain('role="status"');
  expect(html).toContain('aria-live="polite"');
  expect(html).toContain("original files are preserved");
  expect(html).toContain("Unavailable preferences use safe defaults");
  expect(html).toContain("affected sensitive features stay locked");
  expect(html).not.toContain("fresh personal defaults");
  expect(html).toContain("You can save new preferences");
  expect(html).toContain("Dismiss for now");
  expect(html).toContain('aria-label="Dismiss personal data recovery notice for this session"');
  expect(html).toContain("Details remain in Settings");
  expect(html).toContain("Dismissing does not unlock protected data");
  expect(html).not.toContain('role="dialog"');
  expect(html).not.toContain("Retry startup");
});

test("recovery banner offers a Settings route and details stay available independently of dismissal", () => {
  const status = { mode: "service" as const, recoveryRequired: true, canSave: true };
  const banner = renderToStaticMarkup(<PersonalSettingsNotice status={status} onOpenSettings={() => {}} />);
  expect(banner).toContain("Review in Settings");
  const details = renderToStaticMarkup(<PersonalSettingsRecoveryDetails status={status} />);
  expect(details).toContain('aria-label="Personal data recovery"');
  expect(details).toContain("original files are preserved");
  expect(details).toContain("Administrator permission alone cannot unlock another account");
  expect(details).not.toContain("Dismiss");
  expect(status.recoveryRequired).toBe(true);
});

test("an acknowledged episode has a stable identity until the actual recovery or save status changes", () => {
  const status = { mode: "service" as const, recoveryRequired: true, canSave: true };
  const episode = PersonalSettingsNotice({ status });
  expect(PersonalSettingsNotice({ status: { ...status } })?.key).toBe(episode?.key);
  expect(PersonalSettingsNotice({ status: { ...status, canSave: false } })?.key).not.toBe(episode?.key);
  expect(PersonalSettingsNotice({ status: { ...status, recoveryRequired: false } })).toBeNull();
  expect(PersonalSettingsNotice({ status: null })).toBeNull();
});

test("temporary recovery preferences never claim they can be saved", () => {
  const html = renderToStaticMarkup(<PersonalSettingsNotice status={{ mode: "temporary", recoveryRequired: true, canSave: false }} />);
  expect(html).toContain("original files are preserved");
  expect(html).toContain("cannot be saved right now");
  expect(html).not.toContain("You can save new preferences");
});

test("temporary service failure without key loss does not claim sensitive data is locked", () => {
  const html = renderToStaticMarkup(<PersonalSettingsNotice status={{ mode: "temporary", recoveryRequired: false, canSave: false }} />);
  expect(html).toContain("temporarily unavailable");
  expect(html).toContain("cannot be saved right now");
  expect(html).not.toContain("affected sensitive features stay locked");
});

test("healthy service or legacy preferences do not show a recovery notice", () => {
  for (const status of [null, { mode: "service" as const, recoveryRequired: false, canSave: true }, { mode: "legacy" as const, recoveryRequired: false, canSave: true }]) {
    expect(renderToStaticMarkup(<PersonalSettingsNotice status={status} />)).toBe("");
  }
});
