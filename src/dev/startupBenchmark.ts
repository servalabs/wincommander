import {
  PROTECTION_STARTUP_PHASES, REQUIRED_STARTUP_PHASES, STARTUP_SCENARIOS,
  summarizeStartupSamples, validateStartupSample, type StartupSample,
} from "./startupMeasurement";

export interface StartupBenchmarkMetadata {
  freeRevision: string;
  proRevision: string | null;
  freeArtifactHash: string;
  proArtifactHash: string | null;
  machineId: string;
  windowsVersion: string;
  webviewVersion: string;
  capturedAt: string;
  downloadsEntries: number;
  protectionRequired: boolean;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readMetadata(value: unknown): { metadata: StartupBenchmarkMetadata | null; issues: string[] } {
  if (!isRecord(value)) return { metadata: null, issues: ["metadata: expected build and environment details"] };
  const issues: string[] = [];
  const text = (key: string): string => {
    const field = value[key];
    if (typeof field === "string" && field.trim().length > 0 && field.length <= 160) return field;
    issues.push(`metadata.${key}: expected non-empty text of at most 160 characters`);
    return "";
  };
  const hash = (key: string, length: number): string => {
    const field = text(key);
    if (!new RegExp(`^[a-f0-9]{${length}}$`, "i").test(field)) issues.push(`metadata.${key}: expected a ${length}-character hex hash`);
    return field;
  };
  const nullableHash = (key: string, length: number) => value[key] === null ? null : hash(key, length);
  const metadata: StartupBenchmarkMetadata = {
    freeRevision: hash("freeRevision", 40),
    proRevision: nullableHash("proRevision", 40),
    freeArtifactHash: hash("freeArtifactHash", 64),
    proArtifactHash: nullableHash("proArtifactHash", 64),
    machineId: text("machineId"),
    windowsVersion: text("windowsVersion"),
    webviewVersion: text("webviewVersion"),
    capturedAt: text("capturedAt"),
    downloadsEntries: typeof value.downloadsEntries === "number" ? value.downloadsEntries : -1,
    protectionRequired: value.protectionRequired === true,
  };
  if ((metadata.proRevision === null) !== (metadata.proArtifactHash === null)) issues.push("metadata: Pro revision and artifact must both be present or both null");
  if (!Number.isSafeInteger(metadata.downloadsEntries) || metadata.downloadsEntries < 0) issues.push("metadata.downloadsEntries: expected a non-negative safe integer");
  if (typeof value.protectionRequired !== "boolean") issues.push("metadata.protectionRequired: expected a boolean");
  const captured = Date.parse(metadata.capturedAt);
  if (!Number.isFinite(captured) || new Date(captured).toISOString() !== metadata.capturedAt) issues.push("metadata.capturedAt: expected a UTC ISO timestamp including milliseconds");
  if (Object.keys(value).some((key) => !Object.hasOwn(metadata, key))) issues.push("metadata: unknown field");
  return { metadata: issues.length === 0 ? metadata : null, issues };
}

function completenessIssues(sample: StartupSample, metadata: StartupBenchmarkMetadata | null): string[] {
  const issues: string[] = [];
  for (const phase of REQUIRED_STARTUP_PHASES) {
    if (sample.elapsedMs[phase] === undefined) issues.push(`${phase}: missing timing`);
  }
  if (sample.elapsedMs.process_start !== undefined && sample.elapsedMs.process_start !== 0) issues.push("process_start: expected zero clock origin");
  const outcome = PROTECTION_STARTUP_PHASES.find((phase) => sample.elapsedMs[phase] !== undefined);
  if (!outcome) issues.push("protection: missing outcome timing");
  else if (outcome === "protection_failed") issues.push("protection: failed; cannot qualify a successful startup");
  else if (metadata && outcome !== (metadata.protectionRequired ? "protection_required_ready" : "protection_not_required")) {
    issues.push("protection: outcome contradicts configured requirement");
  }
  const protectionElapsed = outcome ? sample.elapsedMs[outcome] : undefined;
  if (protectionElapsed !== undefined && sample.elapsedMs.background_idle !== undefined && protectionElapsed > sample.elapsedMs.background_idle) {
    issues.push("protection: outcome occurs after the measured startup interval");
  }
  if (sample.scenario === "downloads-50k" && metadata && metadata.downloadsEntries !== 50_000) issues.push("downloads-50k: expected exactly 50000 fixture entries");
  return issues;
}

export function createStartupBenchmarkReport(input: unknown) {
  const envelope = isRecord(input) ? input : null;
  if (envelope && envelope.schemaVersion !== 1) throw new Error("schemaVersion: expected 1");
  const samples: unknown = Array.isArray(input) ? input : envelope?.samples;
  if (!Array.isArray(samples) || samples.length === 0) throw new Error("samples: expected a non-empty array");
  const { metadata, issues: metadataIssues } = readMetadata(envelope?.metadata);
  const accepted: StartupSample[] = [];
  const sampleIssues: { index: number; status: "rejected" | "incomplete"; issues: string[] }[] = [];
  let completeSamples = 0;
  for (const [index, candidate] of samples.entries()) {
    const failures = validateStartupSample(candidate);
    if (failures.length) {
      sampleIssues.push({ index, status: "rejected", issues: failures });
      continue;
    }
    // The unknown JSON crosses the typed boundary only after runtime validation.
    const sample = candidate as StartupSample;
    accepted.push(sample);
    const issues = completenessIssues(sample, metadata);
    if (issues.length || metadataIssues.length) sampleIssues.push({ index, status: "incomplete", issues });
    else completeSamples += 1;
  }
  const summaries = summarizeStartupSamples(accepted);
  return {
    schemaVersion: 1,
    reportOnly: true,
    dataComplete: completeSamples > 0 && completeSamples === samples.length,
    metadata,
    metadataIssues,
    samples: samples.length,
    acceptedSamples: accepted.length,
    rejectedSamples: samples.length - accepted.length,
    completeSamples,
    incompleteSamples: accepted.length - completeSamples,
    sampleIssues,
    summaries,
    unmeasuredScenarios: STARTUP_SCENARIOS.filter((scenario) => !summaries.some((summary) => summary.scenario === scenario)),
    externalGates: ["packaged Windows", "WPR/WPA trace", "reboot", "RDS logon", "Defender/driver/physical-device acceptance"],
  };
}
