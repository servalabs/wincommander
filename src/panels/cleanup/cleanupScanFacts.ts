export type CleanupScanFact = { label: string; value: string };

// Record arrays have their own table; these fields are already represented by
// the result count, coverage, or preview indicators elsewhere in the dialog.
const structuralFields = new Set(['total', 'count', 'sampleCount', 'previewLimit', 'truncated', 'scannedAt', 'scanTime']);
const labels: Record<string, string> = {
  runMruOrder: 'Run history order',
  pcaSvcState: 'Compatibility Assistant service',
  wsearchState: 'Windows Search service',
  spoolerState: 'Print Spooler service',
  totalSizeMB: 'Total size (MB)',
  totalSizeKB: 'Total size (KB)',
  clipboardHistoryDisabled: 'Clipboard history disabled',
  cloudClipboardDisabled: 'Cloud clipboard disabled',
};

export function getCleanupScanFacts(data: unknown, failed = false): CleanupScanFact[] {
  if (!data || typeof data !== 'object' || Array.isArray(data)) return [];
  const root = data as Record<string, unknown>;
  const scanFacts = root.scanFacts && typeof root.scanFacts === 'object' && !Array.isArray(root.scanFacts) ? root.scanFacts as Record<string, unknown> : {};
  return Object.entries({ ...scanFacts, ...root }).flatMap(([key, value]) => {
    if (structuralFields.has(key) || (failed && /^(error|message|errorMessage)$/i.test(key)) || value === null || value === undefined) return [];
    // Status codes are presented by the coverage indicator; descriptive
    // coverage text contains additional source information and must be kept.
    if (key === 'coverage' && typeof value === 'string' && /^(complete|limited|partial)$/i.test(value.trim())) return [];
    if (typeof value !== 'string' && typeof value !== 'number' && typeof value !== 'boolean') return [];
    if (typeof value === 'number' && !Number.isFinite(value)) return [];
    if (typeof value === 'string' && !value.trim()) {
      return key === 'runMruOrder' ? [{ label: labels.runMruOrder, value: '(empty)' }] : [];
    }
    const label = labels[key] ?? key.replace(/([a-z0-9])([A-Z])/g, '$1 $2').replace(/[_-]+/g, ' ').replace(/^./, c => c.toUpperCase());
    return [{ label, value: typeof value === 'boolean' ? (value ? 'Yes' : 'No') : String(value) }];
  });
}
