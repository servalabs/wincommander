// src/lib/stegoBackupValidation.ts
//
// Form rules and failure translation for Stego Backup. Split from
// stegoBackup.ts so each file keeps one job: that one models sizes and
// capacity, this one decides what blocks submission and what a raw handler
// error means in plain language.
//
// The old component validated presence only ("…are all required") and pushed
// every backend string straight at the user through the title-bar bell.

import {
  MAX_PATH_LENGTH,
  MIN_PASSWORD_LENGTH,
  FREE_SPACE_ROUNDING_BYTES,
  formatBytes,
  isSamePath,
  isSupportedCarrier,
  normalizeContainerOutputPath,
  parseContainerSize,
  requiredFreeBytes,
  toBackendSize,
  type SizeUnit,
} from "./stegoBackup";

export type StegoField = "carrier" | "output" | "size" | "password" | "destination";

export interface StegoIssue {
  field: StegoField;
  message: string;
}

export interface CreateFormInput {
  carrierPath: string;
  outputPath: string;
  sizeRaw: string;
  sizeUnit: SizeUnit;
  password: string;
  passwordConfirm: string;
  /** Free space on the output volume, or null when it has not been read. */
  destinationFreeBytes: number | null;
  /** Carrier size when something can measure it; null otherwise. */
  carrierBytes: number | null;
}

export interface CreateFormVerdict {
  errors: StegoIssue[];
  warnings: StegoIssue[];
  containerMb: number | null;
  /** Ready for `createStegoMp4({ size })` — carries the mandatory unit suffix. */
  backendSize: string | null;
  canSubmit: boolean;
}

const CARRIER_FORMATS = "MP4, M4V or MOV";

export function validateCreateForm(input: CreateFormInput): CreateFormVerdict {
  const errors: StegoIssue[] = [];
  const warnings: StegoIssue[] = [];

  if (!input.carrierPath.trim()) {
    errors.push({ field: "carrier", message: "Choose the video that will carry the hidden backup." });
  } else if (!isSupportedCarrier(input.carrierPath)) {
    errors.push({ field: "carrier", message: `The carrier has to be a ${CARRIER_FORMATS} video.` });
  } else if (input.carrierBytes === 0) {
    errors.push({ field: "carrier", message: "That carrier file is empty, so it cannot pass as a video." });
  }

  if (!input.outputPath.trim()) {
    errors.push({ field: "output", message: "Choose where to save the video that carries the backup." });
  } else if (isSamePath(input.carrierPath, input.outputPath)) {
    errors.push({
      field: "output",
      message: "Save to a different file — writing over the carrier destroys the original video.",
    });
  } else if (!isSupportedCarrier(input.outputPath)) {
    warnings.push({
      field: "output",
      message: `The output is still a playable video, so give it a ${CARRIER_FORMATS} name or players will refuse it.`,
    });
  }

  const size = parseContainerSize(input.sizeRaw, input.sizeUnit);
  if (size.error) errors.push({ field: "size", message: size.error });

  errors.push(...passwordIssues(input.password, input.passwordConfirm));

  if (size.mb != null && input.destinationFreeBytes != null) {
    const needed = requiredFreeBytes(size.mb, input.carrierBytes);
    if (needed > input.destinationFreeBytes + FREE_SPACE_ROUNDING_BYTES) {
      errors.push({
        field: "destination",
        message: `Not enough room where the video is going — it needs about ${formatBytes(needed)} and only ${formatBytes(input.destinationFreeBytes)} is free.`,
      });
    }
  }

  warnings.push(...longPathWarnings([
    { field: "carrier", path: input.carrierPath },
    { field: "output", path: input.outputPath },
  ]));

  return {
    errors,
    warnings,
    containerMb: size.mb,
    backendSize: size.mb == null ? null : toBackendSize(size.mb),
    canSubmit: errors.length === 0,
  };
}

function passwordIssues(password: string, confirm: string): StegoIssue[] {
  if (!password) {
    return [{ field: "password", message: "Set a password — nothing can open the volume without it." }];
  }
  if (!password.trim()) {
    return [{ field: "password", message: "That password is only spaces. Type a real one." }];
  }
  if (password.length < MIN_PASSWORD_LENGTH) {
    return [
      {
        field: "password",
        message: `Use at least ${MIN_PASSWORD_LENGTH} characters — a hidden volume can be attacked offline, forever.`,
      },
    ];
  }
  if (password !== confirm) {
    return [{ field: "password", message: "The two passwords do not match." }];
  }
  return [];
}

function longPathWarnings(entries: { field: StegoField; path: string }[]): StegoIssue[] {
  return entries
    .filter((entry) => entry.path.trim().length > MAX_PATH_LENGTH)
    .map((entry) => ({
      field: entry.field,
      message: `That path is longer than ${MAX_PATH_LENGTH} characters — some Windows tools refuse those. A shorter folder is safer.`,
    }));
}

export interface ExtractFormInput {
  inputPath: string;
  outputPath: string;
}

export interface ExtractFormVerdict {
  errors: StegoIssue[];
  warnings: StegoIssue[];
  /** `.hc` enforced so the recovered file is mountable; null when nothing was chosen. */
  normalizedOutputPath: string | null;
  canSubmit: boolean;
}

export function validateExtractForm(input: ExtractFormInput): ExtractFormVerdict {
  const errors: StegoIssue[] = [];
  const warnings: StegoIssue[] = [];

  if (!input.inputPath.trim()) {
    errors.push({ field: "carrier", message: "Choose the video that has a backup hidden inside it." });
  } else if (!isSupportedCarrier(input.inputPath)) {
    errors.push({ field: "carrier", message: `Only a ${CARRIER_FORMATS} video can hold a hidden backup.` });
  }

  const normalizedOutputPath = input.outputPath.trim()
    ? normalizeContainerOutputPath(input.outputPath)
    : null;

  if (!normalizedOutputPath) {
    errors.push({ field: "output", message: "Choose where to write the recovered container." });
  } else if (isSamePath(input.inputPath, normalizedOutputPath)) {
    errors.push({
      field: "output",
      message: "Recovering onto the video itself would destroy it — choose another file.",
    });
  } else if (normalizedOutputPath !== input.outputPath.trim()) {
    warnings.push({
      field: "output",
      message: "Saving with a .hc ending so VeraCrypt offers to mount it.",
    });
  }

  warnings.push(...longPathWarnings([
    { field: "carrier", path: input.inputPath },
    { field: "output", path: input.outputPath },
  ]));

  return { errors, warnings, normalizedOutputPath, canSubmit: errors.length === 0 };
}

/**
 * Which issues the form may show yet. Required-field complaints must not
 * appear before the user has either filled that field or pressed the button,
 * otherwise an untouched form opens covered in red.
 */
export function visibleIssues(
  issues: StegoIssue[],
  filledFields: StegoField[],
  attempted: boolean,
): StegoIssue[] {
  if (attempted) return issues;
  return issues.filter((issue) => filledFields.includes(issue.field));
}

export interface StegoFailure {
  headline: string;
  hint: string;
  /** Untouched handler text, kept for a details disclosure. */
  raw: string;
}

interface FailureRule {
  match: RegExp;
  headline: string;
  hint: string;
}

// Ordered: the most specific cause wins, because handler messages often carry
// several clauses ("Command: …", diagnostic logs) glued together upstream.
const FAILURE_RULES: FailureRule[] = [
  {
    match: /entitlement required|PRO_NOT_INSTALLED|licen[cs]e/i,
    headline: "Hidden video backup needs WinCommander Pro.",
    hint: "Activate a licence or start the free trial, then try again — nothing was written.",
  },
  {
    match: /wcstego|no hidden|no payload|marker|trailer/i,
    headline: "No hidden backup was found in that video.",
    hint: "Either it is an ordinary video, or it was re-encoded on the way here. Sending the file through WhatsApp, YouTube or Google Photos rewrites it and strips the payload.",
  },
  {
    match: /truncat|corrupt|unexpected end|not a valid|invalid mp4|ffprobe|moov/i,
    headline: "That video is damaged or is not really an MP4.",
    hint: "Try the original file rather than a copy that was cut, resumed or partially downloaded.",
  },
  {
    match: /not enough space|no space left|disk full|insufficient (disk|space)/i,
    headline: "The drive ran out of space part-way through.",
    hint: "The output holds the whole carrier plus the hidden volume. Free some space or pick another drive, then run it again.",
  },
  {
    match: /access is denied|permission denied|unauthorized|read-?only|being used by another process/i,
    headline: "Windows refused to write the output file.",
    hint: "The destination may be read-only, on a locked USB stick, or the file may be open in a player. Pick a folder you own, such as Documents.",
  },
  {
    match: /cannot find|not found|no such file|does not exist/i,
    headline: "One of the files was not there any more.",
    hint: "The carrier or the destination folder may have been moved, renamed or unplugged since you picked it.",
  },
  {
    match: /password|passphrase|decrypt/i,
    headline: "The password did not unlock the container.",
    hint: "Passwords are case-sensitive and there is no recovery. Check Caps Lock and your keyboard layout. Recovering the container needs no password — only mounting it does.",
  },
  {
    match: /veracrypt|engine|not installed/i,
    headline: "The encryption engine is not available.",
    hint: "Install the encryption engine from Secure Storage, then try again.",
  },
];

/** Turn a handler/Windows error into something a person can act on. */
export function explainStegoFailure(raw: string, operation: "create" | "extract"): StegoFailure {
  const text = (raw ?? "").trim();
  const rule = FAILURE_RULES.find((candidate) => candidate.match.test(text));
  if (rule) return { headline: rule.headline, hint: rule.hint, raw: text };

  return {
    headline:
      operation === "create"
        ? "The hidden backup was not created."
        : "The hidden container was not recovered.",
    hint: "Nothing usable was written. The details below are the exact message from the engine.",
    raw: text || "The engine gave no reason.",
  };
}
