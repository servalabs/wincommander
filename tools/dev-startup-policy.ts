/** Determine destructive setup actions before starting a development server. */
export function vitePortOwner(output: string, status: number | null, requireInspection: boolean): string | null {
  if (status !== 0) {
    if (requireInspection) throw new Error("Could not inspect port 1420; existing processes were preserved.");
    return null;
  }
  return output.split(/\r?\n/)
    .map(line => line.match(/:1420\s+\S+\s+LISTENING\s+(\d+)\s*$/i))
    .find((match): match is RegExpMatchArray => match !== null)?.[1] ?? null;
}

export function developmentStartupPolicy(options: {
  preserveSessions: boolean;
  freeOnly: boolean;
  precleaned: boolean;
  portOwner: string | null;
}): { cleanProcesses: boolean; releasePort: boolean; noServiceRestart: boolean } {
  if (options.preserveSessions && (!options.freeOnly || options.precleaned)) {
    throw new Error("Session-preserving startup requires --free and cannot follow pre-cleaned startup.");
  }
  if (options.preserveSessions && options.portOwner !== null) {
    throw new Error("Port 1420 is already in use; its listener was preserved. Use the existing frontend server or close it yourself before starting another.");
  }
  return {
    cleanProcesses: !options.precleaned && !options.preserveSessions,
    releasePort: !options.preserveSessions,
    noServiceRestart: options.preserveSessions,
  };
}
