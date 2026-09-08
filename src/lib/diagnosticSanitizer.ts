/**
 * Legacy logs predate the structured diagnostics privacy contract. Their free
 * text therefore has no approved data classification and must never cross the
 * Diagnostic Center boundary. Keep only deterministic record metadata.
 */
export function safeLegacyDiagnosticSource(source: string): string {
  const normalizedSource = source.trim().toUpperCase();
  return /^[A-Z0-9_-]{1,20}$/.test(normalizedSource) ? normalizedSource : "LEGACY";
}

export function safeLegacyDiagnosticSummary(source: string, level: string): string {
  const normalizedLevel = level.trim().toUpperCase();
  const sourceLabel = safeLegacyDiagnosticSource(source);
  const levelLabel = /^[A-Z]{1,12}$/.test(normalizedLevel) ? normalizedLevel : "EVENT";
  return `Legacy ${sourceLabel} ${levelLabel} record`;
}

/**
 * Copy/export is intentionally derived from already-safe fields. `message` is
 * accepted only to make accidental use explicit; it is never returned.
 */
export function sanitizeLegacyDiagnosticMessage(
  _message: string,
  source: string,
  level: string,
): string {
  return safeLegacyDiagnosticSummary(source, level);
}
