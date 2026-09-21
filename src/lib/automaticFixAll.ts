import type { ScanFinding } from "../components/startup/WizardAnimations";

/**
 * Returns the findings that may be applied without a dashboard click.
 *
 * `safeDefault` is the registry's explicit "safe for every user" marker.
 * A drift finding represents a setting the person has already chosen, so one
 * such finding makes the whole batch manual.  That prevents the automatic
 * preference from silently changing a deliberate per-setting choice.
 */
export function automaticFixAllCandidates(
  findings: readonly ScanFinding[],
  ignoredFindingIds: readonly string[],
): ScanFinding[] {
  const ignored = new Set(ignoredFindingIds);
  const visible = findings.filter((finding) => !ignored.has(finding.id));

  if (visible.some((finding) => finding.drift)) return [];

  return visible.filter((finding) => finding.safeDefault === true);
}

/** A stable key prevents retry loops when an automatic operation fails. */
export function automaticFixAllFingerprint(findings: readonly ScanFinding[]): string {
  return findings
    .map((finding) => `${finding.id}:${finding.targetChecked ?? ""}`)
    .sort()
    .join("|");
}
