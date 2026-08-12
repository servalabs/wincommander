import { expect, test } from "bun:test";
import {
  browserExtensionSettingKey,
  isBrowserExtensionEnabled,
} from "./browserExtensions";

test("browser extension preferences do not cross browser boundaries", () => {
  const settings = {
    [browserExtensionSettingKey("Google Chrome", "privacy-badger")]: false,
    [browserExtensionSettingKey("Microsoft Edge", "privacy-badger")]: true,
  };

  expect(isBrowserExtensionEnabled(settings, "Google Chrome", "privacy-badger")).toBe(false);
  expect(isBrowserExtensionEnabled(settings, "Microsoft Edge", "privacy-badger")).toBe(true);
});

test("browser-specific preferences override legacy global preferences", () => {
  const settings = {
    "privacy-badger": false,
    [browserExtensionSettingKey("Firefox", "privacy-badger")]: true,
  };

  expect(isBrowserExtensionEnabled(settings, "Firefox", "privacy-badger")).toBe(true);
  expect(isBrowserExtensionEnabled(settings, "Google Chrome", "privacy-badger")).toBe(false);
});

test("unset browser extension preferences remain enabled", () => {
  expect(isBrowserExtensionEnabled(undefined, "Brave", "sponsorblock")).toBe(true);
});
