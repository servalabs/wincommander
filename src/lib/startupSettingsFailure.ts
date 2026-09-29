import { getStartupSettingsRecoveryMessage } from "./startupHydration";

/** Expose the failure category, never native paths, identities or stored values. */
export function getStartupSettingsFailureMessage(error: unknown): string {
  const recovery = getStartupSettingsRecoveryMessage(error);
  if (recovery) return recovery;
  const message = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  if (/service rejected request:|service identity could not be verified|service.*(?:denied|authentication)|personal settings.*(?:denied|invalid|missing)/i.test(message)) {
    return "The local WinCommander service could not authorize or read this account's settings. Retry startup. If this continues, ask an administrator to repair the installation. Do not delete your saved settings.";
  }
  if (/access.*denied|permission.*denied|os error 5/i.test(message)) {
    return "Windows denied access to the settings storage. Standard users should be able to open WinCommander; an administrator may need to repair its storage permissions. Do not delete your saved settings.";
  }
  if (/service connect|service connection|service.*timed out|personal settings transport/i.test(message)) {
    return "WinCommander could not reach its local settings service. Wait a moment and retry. If it keeps failing, ask an administrator to repair the installation.";
  }
  if (/decoding failed|decrypt|invalid personal secrets/i.test(message)) {
    return "WinCommander could not unlock the saved settings. The encryption key or saved data could not be verified. Do not delete your settings or encryption keys; restore access to the original data before retrying.";
  }
  if (/deserializ|parse.*settings|settings.*corrupt/i.test(message)) {
    return "WinCommander could not read the saved settings format. Retry startup or repair the installation. Do not delete your saved settings or encryption keys.";
  }
  return "WinCommander could not load this account's settings. Retry startup. If it keeps failing, repair the installation without deleting your saved settings.";
}
