// User-facing names for decoy tripwire events. Keep a read/open distinct from
// a filesystem change: the first proves access; the second proves only that
// the enrolled file changed.
export type DecoyEventKind = "opened" | "modified" | "renamed" | "removed" | "unknown";

export function decoyEventKind(kind: string | null | undefined): DecoyEventKind {
  switch (kind?.trim().toLowerCase()) {
    // `read` was the previous wire value. Keep old queued history legible;
    // newly recorded audit events use the clearer `opened` value.
    case "opened":
    case "read": return "opened";
    case "modified": return "modified";
    case "renamed": return "renamed";
    case "removed": return "removed";
    default: return "unknown";
  }
}

export function decoyEventLabel(kind: string | null | undefined): string {
  switch (decoyEventKind(kind)) {
    case "opened": return "Opened or read";
    case "modified": return "Changed";
    case "renamed": return "Renamed";
    case "removed": return "Removed";
    default: return "Activity detected";
  }
}

export function decoyEventToast(kind: string | null | undefined, fileName: string): string {
  const label = decoyEventLabel(kind);
  if (decoyEventKind(kind) === "opened") {
    return `Decoy file opened or read: ${fileName}. This is an access event; review this device now.`;
  }
  return `Decoy file ${label.toLowerCase()}: ${fileName}. This is a file-change event, not proof that someone opened it.`;
}
