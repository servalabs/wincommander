export type SoftTimeoutResult<T> =
  | { status: "completed"; value: T }
  | { status: "timed-out" };

export function waitForSoftTimeout<T>(
  operation: Promise<T>,
  timeoutMs: number,
): Promise<SoftTimeoutResult<T>> {
  return new Promise((resolve, reject) => {
    let settled = false;
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      resolve({ status: "timed-out" });
    }, timeoutMs);

    operation.then(
      (value) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        resolve({ status: "completed", value });
      },
      (error: unknown) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        reject(error);
      },
    );
  });
}
