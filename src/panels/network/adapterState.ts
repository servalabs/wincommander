import type { AdapterMacResult, PhysicalNetworkAdapter } from "../../hooks/useBackend";

export type AdapterAction = "randomize" | "factory" | "recover";

export function formatAdapterMac(value: string | null | undefined): string {
    const normalized = value?.replace(/[:-]/g, "").toUpperCase();
    return normalized?.match(/^[0-9A-F]{12}$/) ? normalized.match(/.{2}/g)!.join(":") : "Unknown";
}

export function adapterMacDescription(adapter: PhysicalNetworkAdapter): string {
    if (adapter.configurationError) return "Configuration unavailable";
    if (adapter.configuredMac && adapter.currentMac?.replace(/[:-]/g, "").toUpperCase() !== adapter.configuredMac.replace(/[:-]/g, "").toUpperCase()) {
        return "Configured address is not currently applied";
    }
    if (adapter.macState === "factory") return "Matches permanent address";
    if (adapter.macState === "changed") return "Differs from permanent address";
    return "Permanent address comparison unavailable";
}

export function adapterOperationMessage(result: AdapterMacResult, action: AdapterAction): { verified: boolean; message: string } {
    if (result.status !== "verified" || !result.observedMac) {
        return { verified: false, message: result.message || "Windows did not verify the change. Refresh before trying again." };
    }
    const change = action === "randomize" ? "Random MAC verified" : action === "recover" ? "Previous MAC verified" : "Factory MAC verified";
    const link = result.linkStatus === "Up" ? "Network link is up." : `Network link: ${result.linkStatus || "unknown"}.`;
    return { verified: true, message: `${change}: ${formatAdapterMac(result.observedMac)}. ${link}` };
}
