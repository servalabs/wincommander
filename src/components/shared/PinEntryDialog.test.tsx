// src/components/shared/PinEntryDialog.test.tsx
//
// No React Testing Library / jsdom is wired into this repo's test runner
// (see other *.test.tsx files — they use bun:test + renderToStaticMarkup,
// which cannot simulate clicks/typing). So this covers what's actually
// testable without a DOM:
//   1. validatePin — the same 6-20-ASCII-digit rule the backend enforces
//      (Set-BitLockerTpmPin: `^[0-9]{6,20}$`), including rejection of
//      too-short and non-digit input.
//   2. buildToggleCommandParams — the pure param-merge ToggleSection uses
//      to call the enable/disable command, proving a valid PIN produces
//      exactly { Enable: true, Pin: <pin>, Drive: "C:" } and OFF produces
//      { Enable: false, Drive: "C:" } with no Pin.
//   3. An SSR smoke render of the open dialog (matches the convention in
//      redesignPrimitives.test.tsx) confirming the PIN field is a real
//      accessible text node, not CSS-generated content.

import { describe, expect, test } from "bun:test";
import type { ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import PinEntryDialog, { PinEntryDialogBody, validatePin } from "./PinEntryDialog";
import { Dialog } from "@/components/ui/dialog";
import { buildToggleCommandParams } from "../../types/toggles";

// DialogTitle/DialogDescription read Radix's Dialog.Root context, so the
// stateless body must be rendered inside an (open) Dialog root to avoid
// "must be used within Dialog" — the root's own Portal-bound content is
// what's empty under SSR, not the context provider itself.
function withDialogRoot(children: ReactNode) {
  return <Dialog open>{children}</Dialog>;
}

describe("validatePin", () => {
  test("rejects empty input", () => {
    expect(validatePin("")).toBe("Enter a PIN");
  });

  test("rejects PINs shorter than 6 digits", () => {
    expect(validatePin("1234")).toBe("PIN must be 6-20 digits");
  });

  test("rejects PINs longer than 20 digits", () => {
    expect(validatePin("1".repeat(21))).toBe("PIN must be 6-20 digits");
  });

  test("rejects non-digit characters", () => {
    expect(validatePin("12345a")).toBe("PIN must contain only digits");
  });

  test("accepts a 6-digit PIN", () => {
    expect(validatePin("123456")).toBeNull();
  });

  test("accepts a 20-digit PIN", () => {
    expect(validatePin("1".repeat(20))).toBeNull();
  });
});

describe("buildToggleCommandParams (bitlockerTpmPinEnforce wiring)", () => {
  const toggle = { extraCmdParams: { Drive: "C:" }, enableParamName: "Enable" };

  test("turning ON with a PIN sends { Enable: true, Pin, Drive: 'C:' }", () => {
    expect(buildToggleCommandParams(toggle, true, { Pin: "482913" })).toEqual({
      Drive: "C:",
      Pin: "482913",
      Enable: true,
    });
  });

  test("turning OFF sends { Enable: false, Drive: 'C:' } with no Pin", () => {
    expect(buildToggleCommandParams(toggle, false)).toEqual({
      Drive: "C:",
      Enable: false,
    });
  });
});

// PinEntryDialogBody is the stateless markup PinEntryDialog mounts inside
// <DialogContent>. Radix's Dialog portal renders empty under
// renderToStaticMarkup (no DOM), so the modal's actual content — the "turning
// the toggle on shows the PIN modal" surface — is exercised here directly,
// the same way the wrapped component renders it once Radix opens the portal
// client-side.
describe("PinEntryDialogBody (the PIN modal content)", () => {
  test("shows the field with a real accessible label — not CSS content", () => {
    const html = renderToStaticMarkup(
      withDialogRoot(
        <PinEntryDialogBody
          toggleLabel="BitLocker TPM+PIN"
          description="Require a PIN on every boot"
          pin=""
          validationError={null}
          busy={false}
          inputId="pin"
          errorId="pin-error"
          onPinChange={() => {}}
          onBlur={() => {}}
          onEnter={() => {}}
          onCancel={() => {}}
          onConfirm={() => {}}
        />,
      ),
    );

    expect(html).toContain("Set a PIN for BitLocker TPM+PIN");
    expect(html).toContain("Require a PIN on every boot");
    // Real text node for the field label, not ::before/::after content.
    expect(html).toContain(">PIN<");
    expect(html).toContain('type="password"');
  });

  test("an invalid PIN (too short) surfaces the rejection and disables Turn on", () => {
    const html = renderToStaticMarkup(
      withDialogRoot(
        <PinEntryDialogBody
          toggleLabel="BitLocker TPM+PIN"
          description="Require a PIN on every boot"
          pin="123"
          validationError={validatePin("123")}
          busy={false}
          inputId="pin"
          errorId="pin-error"
          onPinChange={() => {}}
          onBlur={() => {}}
          onEnter={() => {}}
          onCancel={() => {}}
          onConfirm={() => {}}
        />,
      ),
    );

    expect(html).toContain("PIN must be 6-20 digits");
    expect(html).toContain('aria-invalid="true"');
    // The "Turn on" button carries the disabled attribute while invalid.
    expect(html).toMatch(/Turn on<\/button>/);
    expect(html).toContain("disabled=\"\"");
  });
});

describe("PinEntryDialog (SSR smoke)", () => {
  test("renders nothing when closed", () => {
    const html = renderToStaticMarkup(
      <PinEntryDialog
        isOpen={false}
        toggleLabel="BitLocker TPM+PIN"
        description="Require a PIN on every boot"
        onConfirm={() => {}}
        onCancel={() => {}}
      />,
    );

    expect(html).not.toContain("Set a PIN for");
  });
});
