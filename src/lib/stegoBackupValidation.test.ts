import { describe, it, expect } from "bun:test";
import {
  explainStegoFailure,
  validateCreateForm,
  validateExtractForm,
  visibleIssues,
  type CreateFormInput,
  type StegoField,
  type StegoIssue,
} from "./stegoBackupValidation";
import { FREE_SPACE_ROUNDING_BYTES, requiredFreeBytes } from "./stegoBackup";

const createInput = (over: Partial<CreateFormInput> = {}): CreateFormInput => ({
  carrierPath: "C:\\Videos\\holiday.mp4",
  outputPath: "C:\\Videos\\holiday-backup.mp4",
  sizeRaw: "20",
  sizeUnit: "M",
  password: "correct horse battery",
  passwordConfirm: "correct horse battery",
  destinationFreeBytes: 50 * 1024 ** 3,
  carrierBytes: null,
  ...over,
});

const on = (issues: StegoIssue[], field: StegoField): string[] =>
  issues.filter((issue) => issue.field === field).map((issue) => issue.message);

describe("validateCreateForm", () => {
  it("a complete form submits and hands the backend a size with its mandatory unit", () => {
    const verdict = validateCreateForm(createInput());
    expect(verdict.errors).toHaveLength(0);
    expect(verdict.canSubmit).toBe(true);
    expect(verdict.containerMb).toBe(20);
    expect(verdict.backendSize).toBe("20M");
  });

  it("blocks a missing carrier with a reason naming the field", () => {
    const verdict = validateCreateForm(createInput({ carrierPath: "" }));
    expect(verdict.canSubmit).toBe(false);
    expect(on(verdict.errors, "carrier")[0]).toContain("carry the hidden backup");
  });

  it("refuses an unsupported carrier format before any backend call", () => {
    const verdict = validateCreateForm(createInput({ carrierPath: "C:\\Videos\\holiday.mkv" }));
    expect(verdict.canSubmit).toBe(false);
    expect(on(verdict.errors, "carrier")[0]).toContain("MP4");
  });

  it("refuses a carrier with zero usable cover once its size is known", () => {
    const verdict = validateCreateForm(createInput({ carrierBytes: 0 }));
    expect(verdict.canSubmit).toBe(false);
    expect(on(verdict.errors, "carrier")[0]).toContain("empty");
  });

  it("refuses to write over the carrier even when the two paths are spelled differently", () => {
    const verdict = validateCreateForm(
      createInput({ carrierPath: "C:\\Videos\\holiday.mp4", outputPath: "c:/videos/holiday.mp4" }),
    );
    expect(verdict.canSubmit).toBe(false);
    expect(on(verdict.errors, "output")[0]).toContain("destroys the original");
  });

  it("only warns when the output is not named like a video, because it still plays", () => {
    const verdict = validateCreateForm(createInput({ outputPath: "C:\\Videos\\backup.bin" }));
    expect(verdict.canSubmit).toBe(true);
    expect(on(verdict.warnings, "output")[0]).toContain("players will refuse it");
  });

  it("blocks a zero size and produces no backend size at all", () => {
    const verdict = validateCreateForm(createInput({ sizeRaw: "0" }));
    expect(verdict.canSubmit).toBe(false);
    expect(verdict.containerMb).toBeNull();
    expect(verdict.backendSize).toBeNull();
  });

  it("blocks a non-numeric size rather than sending a 0 MB volume", () => {
    const verdict = validateCreateForm(createInput({ sizeRaw: "twenty" }));
    expect(verdict.canSubmit).toBe(false);
    expect(verdict.backendSize).toBeNull();
    expect(on(verdict.errors, "size")).toHaveLength(1);
  });

  it("blocks an empty password", () => {
    const verdict = validateCreateForm(createInput({ password: "", passwordConfirm: "" }));
    expect(verdict.canSubmit).toBe(false);
    expect(on(verdict.errors, "password")[0]).toContain("Set a password");
  });

  it("blocks a password made only of spaces", () => {
    const verdict = validateCreateForm(createInput({ password: "        ", passwordConfirm: "        " }));
    expect(verdict.canSubmit).toBe(false);
    expect(on(verdict.errors, "password")[0]).toContain("only spaces");
  });

  it("blocks a password under the eight-character minimum and says the minimum", () => {
    const verdict = validateCreateForm(createInput({ password: "short12", passwordConfirm: "short12" }));
    expect(verdict.canSubmit).toBe(false);
    expect(on(verdict.errors, "password")[0]).toContain("8 characters");
  });

  it("blocks a mismatched confirmation before a volume is created with a typo in its password", () => {
    const verdict = validateCreateForm(createInput({ passwordConfirm: "correct horse bettery" }));
    expect(verdict.canSubmit).toBe(false);
    expect(on(verdict.errors, "password")[0]).toContain("do not match");
  });

  it("accepts a unicode password at the minimum length and rejects one below it", () => {
    expect(validateCreateForm(createInput({ password: "пароль🔒", passwordConfirm: "пароль🔒" })).canSubmit).toBe(true);
    expect(validateCreateForm(createInput({ password: "парол🔒", passwordConfirm: "парол🔒" })).canSubmit).toBe(false);
  });

  it("never trims the password, so surrounding spaces still have to be typed twice", () => {
    expect(validateCreateForm(createInput({ password: "  spaced pw  ", passwordConfirm: "  spaced pw  " })).canSubmit).toBe(true);
    expect(validateCreateForm(createInput({ password: "  spaced pw  ", passwordConfirm: "spaced pw" })).canSubmit).toBe(false);
  });

  it("blocks a volume that cannot fit in the destination's free space", () => {
    const verdict = validateCreateForm(createInput({ sizeRaw: "500", destinationFreeBytes: 10 * 1024 ** 2 }));
    expect(verdict.canSubmit).toBe(false);
    expect(on(verdict.errors, "destination")[0]).toContain("Not enough room");
  });

  it("allows a volume that fits the free space exactly", () => {
    const verdict = validateCreateForm(
      createInput({ sizeRaw: "20", destinationFreeBytes: requiredFreeBytes(20, null) }),
    );
    expect(verdict.canSubmit).toBe(true);
  });

  it("does not block inside the drive readout's rounding margin, but does block beyond it", () => {
    const needed = requiredFreeBytes(20, null);
    const insideMargin = validateCreateForm(
      createInput({ sizeRaw: "20", destinationFreeBytes: needed - FREE_SPACE_ROUNDING_BYTES }),
    );
    expect(insideMargin.canSubmit).toBe(true);

    const pastMargin = validateCreateForm(
      createInput({ sizeRaw: "20", destinationFreeBytes: needed - FREE_SPACE_ROUNDING_BYTES - 1 }),
    );
    expect(pastMargin.canSubmit).toBe(false);
  });

  it("counts the carrier's own bytes against free space once they are known", () => {
    const carrierBytes = 40 * 1024 ** 3;
    const verdict = validateCreateForm(
      createInput({ sizeRaw: "20", carrierBytes, destinationFreeBytes: 20 * 1024 ** 3 }),
    );
    expect(verdict.canSubmit).toBe(false);
    expect(on(verdict.errors, "destination")).toHaveLength(1);
  });

  it("does not judge free space when the drive could not be read", () => {
    const verdict = validateCreateForm(createInput({ sizeRaw: "500", destinationFreeBytes: null }));
    expect(verdict.canSubmit).toBe(true);
    expect(on(verdict.errors, "destination")).toHaveLength(0);
  });

  it("warns about a path past MAX_PATH without blocking a machine that allows long paths", () => {
    const deep = `C:\\${"nested\\".repeat(40)}holiday.mp4`;
    const verdict = validateCreateForm(createInput({ carrierPath: deep }));
    expect(verdict.canSubmit).toBe(true);
    expect(on(verdict.warnings, "carrier")[0]).toContain("260 characters");
  });

  it("reports every problem at once instead of one per attempt", () => {
    const verdict = validateCreateForm(
      createInput({ carrierPath: "", sizeRaw: "abc", password: "", passwordConfirm: "" }),
    );
    expect(verdict.errors).toHaveLength(3);
  });
});

describe("validateExtractForm", () => {
  it("a complete restore form submits with a mountable .hc destination", () => {
    const verdict = validateExtractForm({
      inputPath: "C:\\Videos\\holiday-backup.mp4",
      outputPath: "C:\\Vault\\recovered",
    });
    expect(verdict.canSubmit).toBe(true);
    expect(verdict.normalizedOutputPath).toBe("C:\\Vault\\recovered.hc");
  });

  it("blocks a missing stego video", () => {
    const verdict = validateExtractForm({ inputPath: "", outputPath: "C:\\Vault\\recovered.hc" });
    expect(verdict.canSubmit).toBe(false);
    expect(on(verdict.errors, "carrier")[0]).toContain("hidden inside");
  });

  it("refuses an input format the handler cannot read", () => {
    const verdict = validateExtractForm({ inputPath: "C:\\Videos\\clip.mkv", outputPath: "C:\\Vault\\r.hc" });
    expect(verdict.canSubmit).toBe(false);
    expect(on(verdict.errors, "carrier")[0]).toContain("MP4");
  });

  it("blocks a missing destination", () => {
    const verdict = validateExtractForm({ inputPath: "C:\\Videos\\holiday-backup.mp4", outputPath: "" });
    expect(verdict.canSubmit).toBe(false);
    expect(verdict.normalizedOutputPath).toBeNull();
  });

  it("refuses to recover the container over the stego video itself", () => {
    const verdict = validateExtractForm({
      inputPath: "C:\\Videos\\holiday-backup.hc",
      outputPath: "c:/videos/holiday-backup.hc",
    });
    expect(verdict.canSubmit).toBe(false);
    expect(on(verdict.errors, "output")[0]).toContain("destroy");
  });

  it("explains the added .hc rather than renaming the file silently", () => {
    const verdict = validateExtractForm({
      inputPath: "C:\\Videos\\holiday-backup.mp4",
      outputPath: "C:\\Vault\\recovered",
    });
    expect(on(verdict.warnings, "output")[0]).toContain(".hc");
  });

  it("says nothing when the destination is already a container name", () => {
    const verdict = validateExtractForm({
      inputPath: "C:\\Videos\\holiday-backup.mp4",
      outputPath: "C:\\Vault\\recovered.hc",
    });
    expect(verdict.warnings).toHaveLength(0);
  });
});

describe("visibleIssues", () => {
  const issues: StegoIssue[] = [
    { field: "carrier", message: "pick a carrier" },
    { field: "size", message: "bad size" },
  ];

  it("hides a complaint about a field the user has not touched yet", () => {
    expect(visibleIssues(issues, ["size"], false)).toEqual([{ field: "size", message: "bad size" }]);
  });

  it("shows everything once the button has been pressed", () => {
    expect(visibleIssues(issues, [], true)).toHaveLength(2);
  });
});

describe("explainStegoFailure", () => {
  it("turns the entitlement refusal into an upgrade message, not a raw licence string", () => {
    const failure = explainStegoFailure(
      "WinCommander Pro entitlement required for: Create-StegoMp4. Activate a license or start the 16-day free trial.",
      "create",
    );
    expect(failure.headline).toContain("Pro");
    expect(failure.hint).toContain("nothing was written");
  });

  it("explains a wrong password and that recovery itself needs none", () => {
    const failure = explainStegoFailure("VeraCrypt: incorrect password or not a VeraCrypt volume", "extract");
    expect(failure.headline).toContain("password");
    expect(failure.hint).toContain("no password");
  });

  it("explains a truncated or corrupted carrier", () => {
    const failure = explainStegoFailure("unexpected end of file while reading the moov atom", "extract");
    expect(failure.headline).toContain("damaged");
  });

  it("explains a carrier with no payload, including the re-encode trap", () => {
    const failure = explainStegoFailure("WCSTEGO1 marker not found in input", "extract");
    expect(failure.headline).toContain("No hidden backup");
    expect(failure.hint).toContain("WhatsApp");
  });

  it("explains a read-only or locked destination", () => {
    const failure = explainStegoFailure("Access is denied. (os error 5)", "create");
    expect(failure.headline).toContain("refused to write");
    expect(failure.hint).toContain("read-only");
  });

  it("explains running out of disk space mid-write", () => {
    const failure = explainStegoFailure("There is not enough space on the disk. (os error 112)", "create");
    expect(failure.headline).toContain("ran out of space");
  });

  it("explains a carrier or destination that has gone missing", () => {
    const failure = explainStegoFailure("The system cannot find the file specified. (os error 2)", "create");
    expect(failure.headline).toContain("not there");
  });

  it("prefers the missing-payload explanation over the password one when both words appear", () => {
    const failure = explainStegoFailure("no hidden payload; password prompt skipped", "extract");
    expect(failure.headline).toContain("No hidden backup");
  });

  it("names the operation it could not finish when the cause is unrecognised", () => {
    expect(explainStegoFailure("exit code 9009", "create").headline).toContain("not created");
    expect(explainStegoFailure("exit code 9009", "extract").headline).toContain("not recovered");
  });

  it("always keeps the engine's own words for the details disclosure", () => {
    const raw = "Command: Create-StegoMp4\nexit code 9009";
    expect(explainStegoFailure(raw, "create").raw).toBe(raw);
    expect(explainStegoFailure("", "create").raw).toContain("no reason");
  });
});
