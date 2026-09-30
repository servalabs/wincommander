// A refresh signal, never an authorization claim or a carrier for private data.
export const FLEET_VAULTS_CHANGED_EVENT = "fleet-vaults-changed";

export async function afterVaultMutation<T>(operation: () => Promise<T>, target: EventTarget = window): Promise<T> {
  try {
    return await operation();
  } finally {
    // A failed bulk operation can still have changed some drives.
    target.dispatchEvent(new Event(FLEET_VAULTS_CHANGED_EVENT));
  }
}
