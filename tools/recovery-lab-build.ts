/** Explicit local-only build selection; UI and native components must agree. */
export function recoveryLabEnabled(env: Record<string, string | undefined>): boolean {
  const native = env.WINCMD_FLEET_RECOVERY_LAB === "1";
  const ui = env.VITE_FLEET_RECOVERY_LAB === "1";
  if (native !== ui) {
    throw new Error("Recovery lab build requires both WINCMD_FLEET_RECOVERY_LAB=1 and VITE_FLEET_RECOVERY_LAB=1, or neither.");
  }
  return native;
}

/** Keep forwarded Cargo options from bypassing the paired lab switches. */
export function validateRecoveryLabCargoArgs(args: string[], lab: boolean): void {
  if (lab) return;
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === "--all-features") throw new Error("All Cargo features require the paired recovery lab switches.");
    let features: string | undefined;
    if (arg === "--features" || arg === "-F") features = args[i + 1];
    else if (arg.startsWith("--features=")) features = arg.slice("--features=".length);
    else if (arg.startsWith("-F") && arg.length > 2) features = arg.slice(2).replace(/^=/, "");
    if (features?.split(/[\s,]+/).some((feature) => feature.split("/").pop() === "fleet-recovery-lab")) {
      throw new Error("The recovery lab Cargo feature requires both paired lab switches.");
    }
  }
}
