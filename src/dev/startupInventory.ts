export const STARTUP_TASK_INVENTORY = [
  { id: "settings-cache", owner: "AppContext", priority: "S-P1", timeoutMs: 1_500, singleFlight: "settings-cache" },
  { id: "startup-status", owner: "AppContext", priority: "S-P2", timeoutMs: 20_000, singleFlight: "startup-status" },
  { id: "system-probe", owner: "AppContext", priority: "S-P2", timeoutMs: 30_000, singleFlight: "system-probe" },
  { id: "dependencies", owner: "AppContext", priority: "S-P2", timeoutMs: 20_000, singleFlight: "dependencies" },
  { id: "mesh-status", owner: "AppContext", priority: "S-P2", timeoutMs: 8_000, singleFlight: "mesh-status" },
  { id: "app-inventory", owner: "AppContext", priority: "S-P3", timeoutMs: 45_000, singleFlight: "app-inventory" },
  { id: "panel-preload", owner: "App", priority: "S-P3", timeoutMs: 10_000, singleFlight: "panel-preload" },
  { id: "disk-cleanup-preload", owner: "App", priority: "S-P3", timeoutMs: 30_000, singleFlight: "disk-cleanup-preload" },
  { id: "search-preload", owner: "App", priority: "S-P3", timeoutMs: 10_000, singleFlight: "search-preload" },
] as const;

export const STARTUP_FIELD_OWNERSHIP = {
  "ideal.*": "Get-WCSystemProbe:first-run",
  "current.*": "Get-WCSystemProbe:subsequent",
  "current.device.*": "Get-StartupStatus or deferred Get-SystemInfo",
  "current.apps.inventory": "Get-AppInventory",
  "live.cpuUsage|ramUsage|cpuTemp": "LiveMetricsProvider",
} as const;
