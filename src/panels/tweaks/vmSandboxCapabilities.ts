export interface VmCapabilities {
  platform: "windows-client" | "windows-server" | "unknown";
  hyperv: boolean;
  sandbox: boolean;
  sandboxSupported: boolean;
  hypervFeature: string;
}

export function supportsWindowsSandbox(capabilities: VmCapabilities | null): boolean {
  return capabilities?.platform === "windows-client" && capabilities.sandboxSupported === true;
}
