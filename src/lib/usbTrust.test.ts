import { describe, expect, test } from "bun:test";
import { trustScoreTone, formatTrustScore } from "./usbTrust";

describe("USB trust score UI model", () => {
  test("formats the numeric score as a stable 0-100 label", () => {
    expect(formatTrustScore(87)).toBe("87/100");
    expect(formatTrustScore(-10)).toBe("0/100");
    expect(formatTrustScore(130)).toBe("100/100");
  });

  test("uses danger, warning, and success tones by score band", () => {
    expect(trustScoreTone(39)).toBe("danger");
    expect(trustScoreTone(40)).toBe("warning");
    expect(trustScoreTone(69)).toBe("warning");
    expect(trustScoreTone(70)).toBe("success");
  });
});
