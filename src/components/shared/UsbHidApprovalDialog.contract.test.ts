import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const root = decodeURIComponent(new URL("../../../", import.meta.url).pathname).replace(/^\/([A-Za-z]:)/, "$1");
const source = (path: string) => readFileSync(`${root}/${path}`, "utf8");

describe("USB HID visual approval UI contract", () => {
  test("positive approval is challenge-bound and no raw PnP instance is exposed", () => {
    const dialog = source("src/components/shared/UsbHidApprovalDialog.tsx");
    const context = source("src/context/UsbHidApprovalContext.tsx");
    const ipc = source("src/hooks/usbHidApprovalIpc.ts");

    expect(ipc).toContain('"begin_usb_hid_visual_challenge"');
    expect(ipc).toContain('"submit_usb_hid_visual_challenge_digit"');
    expect(ipc).not.toContain('"complete_usb_hid_visual_challenge"');
    expect(ipc).toContain('"block_usb_hid_pending"');
    expect(context).toContain("usbHidApprovalIpc.beginChallenge");
    expect(dialog).not.toContain("approve_usb_hid_once");
    expect(dialog).not.toContain("trust_usb_hid_always");
    expect(dialog).not.toContain("instanceId");
    expect(dialog).toContain('beginChallenge("allowOnce")');
    expect(dialog).toContain('beginChallenge("trustAlways")');
    expect(dialog).toContain("submitVisualChallengeDigit(");
    expect(dialog).not.toContain("expectedChallengeDigit");
  });

  test("the visual confirmation has no keyboard activation path", () => {
    const dialog = source("src/components/shared/UsbHidApprovalDialog.tsx");

    expect(dialog).toContain("event.isTrusted");
    expect(dialog).toContain("event.pointerId");
    expect(dialog).toContain("event.isPrimary");
    expect(dialog).toContain("event.detail === 0");
    expect(dialog).toContain("tabIndex={-1}");
    expect(dialog).toContain("onKeyDown={(event) => event.preventDefault()}");
    expect(dialog).toContain("onKeyUp={(event) => event.preventDefault()}");
    expect(dialog).toContain("session.challenge.keypadLayout.map");
  });

  test("only resolved post-containment events can request attention", () => {
    const context = source("src/context/UsbHidApprovalContext.tsx");
    const ipc = source("src/hooks/usbHidApprovalIpc.ts");
    const rust = source("src-tauri/commander-free/src/lib.rs");

    expect(context).toContain('item.status === "pending" || item.status === "containmentFailed"');
    expect(context).toContain("const unresolved = pendingApprovals(list.items)");
    expect(context).toContain("usbHidApprovalIpc.revealSecurityAlert()");
    expect(ipc).toContain('"reveal_main_window_for_security_alert"');
    expect(rust).toContain("fn reveal_main_window_for_security_alert");
    expect(rust).toContain("reveal_main_window(&app)");
  });
});
