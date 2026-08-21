export interface FlowSettingOption {
  path: string;
  label: string;
  value: unknown;
}

const SECRET_SEGMENT_TOKENS = [
  "pin",
  "hash",
  "seed",
  "keyword",
  "phrase",
  "secret",
  "password",
  "material",
  "token",
];

const NOISE_LEAVES = new Set([
  "lastSeenAt",
  "lastPanel",
  "updatedAt",
  "lastCheckedAt",
  "lastRunAt",
  "proFlows",
]);

const EXCLUDED_PREFIXES = ["app.flows", "app.proFlows", "app.contingency"];

export const DEFAULT_FLOW_SETTING_OPTIONS: FlowSettingOption[] = [
  {
    path: "ideal.privacy.telemetry.windowsDisabled",
    label: "Telemetry protection",
    value: true,
  },
  {
    path: "ideal.privacy.telemetry.locationTrackingDisabled",
    label: "Location protection",
    value: true,
  },
  {
    path: "ideal.privacy.appCapabilities.webcam",
    label: "Camera capability",
    value: "Allow",
  },
  {
    path: "ideal.privacy.appCapabilities.microphone",
    label: "Microphone capability",
    value: "Allow",
  },
];

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function segmentIsSecret(segment: string): boolean {
  const lower = segment.toLowerCase();
  return (
    SECRET_SEGMENT_TOKENS.some((token) => lower.includes(token)) ||
    lower.endsWith("priv") ||
    lower.includes("privkey") ||
    lower.includes("privatekey")
  );
}

export function isFlowSettingPathSafe(path: string): boolean {
  if (!path || EXCLUDED_PREFIXES.some((prefix) => path === prefix || path.startsWith(`${prefix}.`))) {
    return false;
  }
  const segments = path.split(".");
  const leaf = segments.at(-1) ?? "";
  return !NOISE_LEAVES.has(leaf) && !segments.some(segmentIsSecret);
}

function humanize(segment: string): string {
  return segment
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/[_-]+/g, " ")
    .replace(/\b\w/g, (letter) => letter.toUpperCase());
}

export function flowSettingLabel(path: string): string {
  const segments = path.split(".");
  const leaf = humanize(segments.at(-1) ?? path);
  const parent = segments.slice(0, -1).join(".");
  return parent ? `${leaf} — ${parent}` : leaf;
}

function collectLeaves(value: unknown, path: string, output: FlowSettingOption[]): void {
  if (isObject(value)) {
    for (const key of Object.keys(value).sort()) {
      const nextPath = path ? `${path}.${key}` : key;
      collectLeaves(value[key], nextPath, output);
    }
    return;
  }
  if (isFlowSettingPathSafe(path)) {
    output.push({ path, label: flowSettingLabel(path), value });
  }
}

export function buildFlowSettingOptions(settings: unknown): FlowSettingOption[] {
  const byPath = new Map(DEFAULT_FLOW_SETTING_OPTIONS.map((option) => [option.path, option]));
  const discovered: FlowSettingOption[] = [];
  collectLeaves(settings, "", discovered);
  for (const option of discovered) {
    byPath.set(option.path, option);
  }
  return [...byPath.values()].sort((a, b) => a.path.localeCompare(b.path));
}

export function formatFlowSettingValue(value: unknown): string {
  if (value === undefined) return "";
  const encoded = JSON.stringify(value);
  return encoded === undefined ? String(value) : encoded;
}

export function parseFlowSettingValue(text: string): unknown {
  const trimmed = text.trim();
  if (!trimmed) return "";
  try {
    return JSON.parse(trimmed);
  } catch {
    return text;
  }
}
