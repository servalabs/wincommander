import { expect, test } from "bun:test";
import { getStartupSettingsFailureMessage } from "./startupSettingsFailure";

test("settings failures explain safe next steps without exposing native details", () => {
  const cases = [
    ["service rejected request: denied secret-user", "could not authorize"],
    ["Service identity could not be verified", "repair the installation"],
    ["Access is denied. (os error 5) at C:\\private", "storage permissions"],
    ["service connect failed: private-pipe", "local settings service"],
    ["Failed to deserialize settings: private-value", "saved settings format"],
    ["Decoding failed — wrong passphrase or corrupted data", "could not unlock the saved settings"],
    ["SETTINGS_KEY_UNAVAILABLE: private-key", "Restore this Windows account's access"],
  ];
  for (const [native, expected] of cases) {
    for (const input of [native, new Error(native)]) {
      const message = getStartupSettingsFailureMessage(input);
      expect(message).toContain(expected);
      expect(message).not.toContain("private-");
      expect(message).not.toContain("secret-user");
      expect(message).not.toContain("C:\\");
    }
  }
});

test("unknown failures do not invent a permissions or corruption diagnosis", () => {
  for (const input of [null, {}, "unexpected internal detail"]) {
    const message = getStartupSettingsFailureMessage(input);
    expect(message).toContain("Retry startup");
    expect(message).not.toContain("internal detail");
    expect(message).not.toContain("denied");
  }
});
