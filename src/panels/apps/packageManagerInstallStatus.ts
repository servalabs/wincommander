const MANAGER_LABELS: Record<string, string> = {
  winget: "Winget",
  chocolatey: "Chocolatey",
  scoop: "Scoop",
  npm: "npm",
};

export interface OptionalManagerInstallOutcome {
  installed: string[];
  alreadyInstalled: string[];
  errors: string[];
}

export interface OptionalManagerInstallStatus {
  tone: "success" | "warning";
  text: string;
}

/** Build a result message that never treats an empty or failed install as success. */
export function summarizeOptionalManagerInstall(
  result: OptionalManagerInstallOutcome,
): OptionalManagerInstallStatus {
  const installed = displayNames(result.installed);
  const alreadyInstalled = displayNames(result.alreadyInstalled);
  const outcomes = [
    installed.length ? `Installed ${installed.join(" and ")}.` : "",
    alreadyInstalled.length ? `${alreadyInstalled.join(" and ")} already installed.` : "",
  ].filter(Boolean);
  const errors = result.errors.map(formatInstallError);

  if (errors.length) {
    return {
      tone: "warning",
      text: [...outcomes, ...errors].join(" "),
    };
  }
  if (!outcomes.length) {
    return {
      tone: "warning",
      text: "No package manager installation was confirmed. Refresh to check the installed managers.",
    };
  }

  return { tone: "success", text: outcomes.join(" ") };
}

function displayNames(ids: string[]): string[] {
  return ids.map((id) => MANAGER_LABELS[id.toLowerCase()] ?? id);
}

function formatInstallError(error: string): string {
  const separator = error.indexOf(":");
  if (separator > 0) {
    const id = error.slice(0, separator).trim().toLowerCase();
    const label = MANAGER_LABELS[id];
    if (label) {
      const details = error.slice(separator + 1).trim();
      return `${label} could not be installed${details ? `: ${details}` : "."}`;
    }
  }
  return `Could not install package manager: ${error}`;
}
