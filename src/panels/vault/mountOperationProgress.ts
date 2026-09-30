export type MountStage = "checking" | "unlocking" | "verifying" | "refreshing";

export function mountProgressMessage(stage: MountStage, seconds: number, customPim: boolean): string {
  const label: Record<MountStage, string> = {
    checking: "Checking that the selected drive letter is free…",
    unlocking: "Waiting for the Vault service to unlock and mount the container…",
    verifying: "Checking that Windows can open the mounted drive…",
    refreshing: "Refreshing the mounted-volume list…",
  };
  const slow = seconds >= 30
    ? " This is taking longer than expected. No result has been confirmed; do not submit another mount request."
    : "";
  const pim = customPim && stage === "unlocking" ? " A custom PIM can take several minutes." : "";
  return `${label[stage]} ${seconds}s elapsed.${pim}${slow}`;
}

// Only use for observations before dispatch, never to cancel or replay a mount.
export async function waitForMountOptions<T>(read: Promise<T>, milliseconds = 15_000): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([read, new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new Error("vault_mount_options_timeout")), milliseconds);
    })]);
  } finally { clearTimeout(timer); }
}

// A receipt is not the same as successful Windows/inventory readback. These
// observations may time out without undoing (or repeating) the successful mount.
export async function waitForMountReadback<T>(
  read: Promise<T>,
  errorCode: "vault_mount_readback_unconfirmed" | "vault_confirmed_mount_list_unavailable",
  milliseconds = 15_000,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([read, new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new Error(errorCode)), milliseconds);
    })]);
  } finally { clearTimeout(timer); }
}
