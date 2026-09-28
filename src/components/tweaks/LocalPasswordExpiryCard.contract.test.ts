import { describe, expect, test } from "bun:test";
import fs from "node:fs";

const card = fs.readFileSync("src/components/tweaks/LocalPasswordExpiryCard.tsx", "utf8");
const backend = fs.readFileSync("src-tauri/commander-free/scripts/modules/tweaks/local-users.ps1", "utf8");
const router = fs.readFileSync("src-tauri/commander-free/src/backend.rs", "utf8");
const panel = fs.readFileSync("src/panels/tweaks/index.tsx", "utf8");

describe("local account password expiry", () => {
  test("reports the actual mixed local-account count and is available beyond Server SKUs", () => {
    expect(card).toContain("formatPasswordExpirySummary(status)");
    expect(panel).toContain("<LocalPasswordExpiryCard />");
    expect(panel).not.toContain("WindowsServerTab\n                            searchQuery={searchQuery}\n                            handlePostToggle={handlePostToggle}\n                        />\n                        <LocalPasswordExpiryCard");
  });

  test("uses Windows account-flag read-back and a fixed, admin-guarded mutation", () => {
    expect(backend).toContain("Win32_UserAccount -Filter 'LocalAccount = True'");
    expect(backend).toContain("PasswordExpires = [bool]$account.PasswordExpires");
    expect(backend).toContain("Set-LocalUser -SID");
    expect(backend).toContain("Assert-IsAdmin");
    expect(router).toContain('"Get-LocalPasswordExpiryStatus"');
    expect(router).toContain('"Set-LocalPasswordNeverExpires"');
  });

  test("excludes accounts that must not be bulk-modified and warns before reversing", () => {
    for (const field of ["builtIn", "disabled", "service", "externalIdentity"]) {
      expect(backend).toContain(`${field} = 0`);
    }
    expect(card).toContain("window.confirm(message)");
    expect(card).toContain("This does not restore each account's previous setting.");
    expect(card).toContain("Windows was rechecked.");
  });
});
