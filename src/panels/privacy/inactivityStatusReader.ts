/** A timed-out status request is not cancelled; do not accumulate replacements. */
export function createInactivityStatusReader(request: () => Promise<unknown>, timeoutMs = 10_000) {
  let pending: Promise<unknown> | null = null;
  return async (): Promise<unknown> => {
    if (pending) throw new Error("The previous timer status request is still pending.");
    const current = Promise.resolve().then(request);
    pending = current;
    void current.then(() => { if (pending === current) pending = null; }, () => { if (pending === current) pending = null; });
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      return await Promise.race([
        current,
        new Promise<never>((_, reject) => { timer = setTimeout(() => reject(new Error("Timer status request timed out.")), timeoutMs); }),
      ]);
    } finally {
      if (timer !== undefined) clearTimeout(timer);
    }
  };
}
