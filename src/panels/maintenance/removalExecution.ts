// Removal replies contain aggregate counts, so a prior report cannot safely
// identify which individual entries remain after a partial or unknown result.
export async function executeRemoval<T>(remove: () => Promise<T>, invalidateReport: () => void): Promise<T> {
  try {
    return await remove();
  } finally {
    invalidateReport();
  }
}
