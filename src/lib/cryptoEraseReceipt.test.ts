import { describe, expect, it } from "bun:test";
import {
  classifyEraseError,
  formatReceiptForClipboard,
  receiptEscrowAdvice,
  receiptFacts,
  receiptHeadline,
  receiptTone,
} from "./cryptoEraseReceipt";
import type { EraseReceipt } from "../hooks/useBackend";

const receipt = (over: Partial<EraseReceipt>): EraseReceipt => ({
  kind: "bitlocker",
  label: "BitLocker D:",
  action: "bitlocker_protectors_removed",
  status: "erased",
  verified: true,
  escrowWarning: null,
  recoveryProtectorsRemaining: 0,
  keyEvicted: true,
  detail: "All key protectors removed and the volume locked.",
  ...over,
});

describe("receiptTone / receiptHeadline", () => {
  it("maps the caveat status to warning, not success", () => {
    expect(receiptTone("erased_with_caveat")).toBe("warning");
    expect(receiptHeadline("erased_with_caveat")).toContain("recovery is still possible");
  });

  it("maps a clean erase to success", () => {
    expect(receiptTone("erased")).toBe("success");
  });

  it("maps a failure to danger", () => {
    expect(receiptTone("failed")).toBe("danger");
  });
});

describe("receiptFacts", () => {
  it("surfaces method and verification for every kind", () => {
    const labels = receiptFacts(receipt({ kind: "veracrypt", action: "veracrypt_header_destroy" })).map(
      (f) => f.label,
    );
    expect(labels).toEqual(["Method", "Verified"]);
  });

  it("adds the BitLocker-only key-eviction and recovery-key facts", () => {
    const labels = receiptFacts(receipt({})).map((f) => f.label);
    expect(labels).toContain("Key evicted");
    expect(labels).toContain("Recovery keys left");
  });

  it("marks a remaining recovery key as danger", () => {
    const fact = receiptFacts(receipt({ recoveryProtectorsRemaining: 1 })).find(
      (f) => f.label === "Recovery keys left",
    );
    expect(fact?.tone).toBe("danger");
  });

  it("omits the recovery-key row when the backend did not report one", () => {
    const labels = receiptFacts(receipt({ recoveryProtectorsRemaining: null })).map((f) => f.label);
    expect(labels).not.toContain("Recovery keys left");
  });

  it("marks an unverified erase as a warning", () => {
    const fact = receiptFacts(receipt({ verified: false })).find((f) => f.label === "Verified");
    expect(fact?.tone).toBe("warning");
  });

  it("shows a friendly method name instead of the raw action id", () => {
    const fact = receiptFacts(receipt({})).find((f) => f.label === "Method");
    expect(fact?.value).toBe("BitLocker protector removal");
  });
});

describe("receiptEscrowAdvice", () => {
  it("is null when nothing is outstanding", () => {
    expect(receiptEscrowAdvice(receipt({}))).toBeNull();
  });

  it("names the concrete next step when a key is escrowed", () => {
    const advice = receiptEscrowAdvice(receipt({ escrowWarning: "Recovery key backed up to Entra." }));
    expect(advice).toContain("Entra");
    expect(advice).toContain("BitLocker D:");
  });

  it("fires on a remaining protector even without an escrow warning string", () => {
    expect(receiptEscrowAdvice(receipt({ recoveryProtectorsRemaining: 2 }))).not.toBeNull();
  });
});

describe("formatReceiptForClipboard", () => {
  it("includes the status, verification and detail for an audit trail", () => {
    const text = formatReceiptForClipboard(receipt({}), 0);
    expect(text).toContain("Status: erased");
    expect(text).toContain("Independently verified: yes");
    expect(text).toContain("Detail: All key protectors removed");
  });

  it("omits the protector line when the backend did not report one", () => {
    const text = formatReceiptForClipboard(receipt({ recoveryProtectorsRemaining: null }), 0);
    expect(text).not.toContain("Recovery protectors remaining");
  });
});

describe("classifyEraseError", () => {
  it("explains the investigator-mode refusal", () => {
    expect(
      classifyEraseError("Refused: investigator mode does not crypto-erase containers.").title,
    ).toContain("investigator");
  });

  it("keeps the server's exact ack requirement visible", () => {
    const advice = classifyEraseError(
      "Refusing OS/system volume erase: a typed confirmation matching 'C:' is required.",
    );
    expect(advice.hint).toContain("'C:'");
  });

  it("tells the user how to fix a missing container path", () => {
    expect(classifyEraseError("veracrypt target requires a path").hint).toContain("re-mount");
  });

  it("keeps an unknown error visible", () => {
    expect(classifyEraseError("boom").hint).toBe("boom");
  });
});
