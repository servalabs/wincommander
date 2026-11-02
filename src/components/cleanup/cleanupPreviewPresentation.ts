type PreviewRow = { label: string; facts: string[] };

export function cleanupFactValue(fact: string): { label: string; value: string } {
  const separator = fact.indexOf(": ");
  return separator < 0 ? { label: "Details", value: fact } : { label: fact.slice(0, separator), value: fact.slice(separator + 2) };
}

function hasValue(value: string): boolean {
  return !!value.trim() && !["(empty)", "No value", "Unavailable", "—", "[]", "{}"].includes(value.trim());
}

export function cleanupPreviewColumns(rows: readonly PreviewRow[]): string[] {
  const labels = new Set<string>();
  for (const row of rows) for (const fact of row.facts) {
    const { label, value } = cleanupFactValue(fact);
    if (hasValue(value)) labels.add(label);
  }
  return [...labels];
}

export function cleanupCardPreview(rows: readonly PreviewRow[]): string[] {
  const preferred = ["Name", "File", "Location", "Host", "Command", "Value", "Artifact", "Description", "Details"];
  return rows.slice(0, 3).map(row => {
    const facts = row.facts.map(cleanupFactValue).filter(fact => hasValue(fact.value));
    const main = preferred.flatMap(label => facts.filter(fact => fact.label === label))[0] ?? facts[0];
    const secondary = facts.find(fact => fact.label === "Size" && fact.value !== main?.value);
    return [main?.value, secondary?.value].filter(Boolean).join(" · ");
  }).filter(Boolean);
}
