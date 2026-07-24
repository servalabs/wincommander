// F-11 (security-audit-report.md): ESLint 9 flat config.
//
// Starts permissive — only the security-critical rules are errors. Style
// and "code smell" rules are off so the first PR doesn't drown in noise.
// Tighten over time by promoting warnings to errors and adding rules.
//
// Pinned rule of substance:
//   react/no-danger — bans dangerouslySetInnerHTML across the codebase
//   (closes audit F-8). The frontend doesn't use it today; this lint
//   ensures it stays that way.

import tseslint from "@typescript-eslint/eslint-plugin";
import tsparser from "@typescript-eslint/parser";
import react from "eslint-plugin-react";
import reactHooks from "eslint-plugin-react-hooks";

// Files fixed in B5 — "react/no-array-index-key" is an error here so regressions
// are caught at lint time. Other files still have pre-existing violations and are
// not covered until they are refactored (project-wide warn added at that point).
const B5_FIXED_FILES = [
  "src/panels/cleanup/index.tsx",
  "src/components/InvestigateDialog.tsx",
  "src/components/ui/bp.tsx",
  "src/components/EverythingSearchBar.tsx",
  "src/panels/search-files/index.tsx",
  "src/components/shared/TraceDetailDialog.tsx",
];

// A6 (IPC-layering guard): raw Tauri `invoke` must only be called from
// src/hooks/** — everything else should go through a typed hook wrapper
// (see src/hooks/useArgus.ts for the pattern). This is a SHRINKING
// migration backlog, not a permanent allowlist: as each file below is
// migrated to a hook, remove it from this list. Do not add new files here —
// new call sites must add/extend a hook instead.
const LEGACY_RAW_INVOKE_FILES = [
  "src/App.tsx",
  "src/components/BackgroundPollers.tsx",
  "src/components/CustomNotificationWindow.tsx",
  "src/components/ErrorBoundary.tsx",
  "src/components/EverythingSearchBar.tsx",
  "src/components/GlobalCommandPalette.tsx",
  "src/components/InvestigateDialog.tsx",
  "src/components/LicenseGate.tsx",
  "src/components/MetadataScrubberDialog.tsx",
  "src/components/RightSidebar.tsx",
  "src/components/ShredConfirmationDialog.tsx",
  "src/components/Sidebar.tsx",
  "src/components/TitleBar.tsx",
  "src/components/UpdaterStatus.tsx",
  "src/components/dashboard/DashboardSidePanel.tsx",
  "src/components/dashboard/HardwareSpecsCard.tsx",
  "src/components/dashboard/RadarControlStrip.tsx",
  "src/components/dashboard/RecentDownloadsCard.tsx",
  "src/components/settings/VersionManagementCard.tsx",
  "src/components/shared/EmbeddedWebView.tsx",
  "src/components/shared/ManagedPolicyBanner.tsx",
  "src/components/startup/CalculatorGate.tsx",
  "src/components/startup/FirstRunWizard.tsx",
  "src/components/tweaks/managers/StartupManager.tsx",
  "src/context/AppContext.tsx",
  "src/context/AuthModeContext.tsx",
  "src/context/ThemeContext.tsx",
  "src/lib/evidence.ts",
  "src/lib/logger.ts",
  "src/panels/cleanup/DriveWipeDialog.tsx",
  "src/panels/dashboard/index.tsx",
  "src/panels/dev/index.tsx",
  "src/panels/fleet/FleetConnectView.tsx",
  "src/panels/flows/FlowExecutionLog.tsx",
  "src/panels/flows/index.tsx",
  "src/panels/sidecar/index.tsx",
  "src/panels/mesh/VpnKillSwitchSection.tsx",
  "src/panels/network/PortGuardSection.tsx",
  "src/panels/network/WifiGuardSection.tsx",
  "src/panels/privacy/AuthAnomalySection.tsx",
  "src/panels/privacy/CanaryTokensSection.tsx",
  "src/panels/privacy/CheckInTimerSection.tsx",
  "src/panels/privacy/CreateWipeUsbDialog.tsx",
  "src/panels/privacy/DecoyMonitorSection.tsx",
  "src/panels/privacy/DriverHealthSection.tsx",
  "src/panels/privacy/EvidenceVaultSection.tsx",
  "src/panels/privacy/FileWatchTriggerSection.tsx",
  "src/panels/privacy/LockdownWordsSection.tsx",
  "src/panels/privacy/LogViewer.tsx",
  "src/panels/privacy/PanicHotkeyTrigger.tsx",
  "src/panels/privacy/PasteMonitorSection.tsx",
  "src/panels/privacy/PrintActivitySection.tsx",
  "src/panels/privacy/PrivacyShieldCard.tsx",
  "src/panels/privacy/RansomwareMonitorSection.tsx",
  "src/panels/privacy/RemoteAccessMonitorSection.tsx",
  "src/panels/privacy/SessionAssuranceSection.tsx",
  "src/panels/privacy/StartupPinConfig.tsx",
  "src/panels/privacy/UsbDevicesSection.tsx",
  "src/panels/privacy/useScreenCapture.ts",
  "src/panels/productivity/index.tsx",
  "src/panels/runtime-visibility/index.tsx",
  "src/panels/search-files/index.tsx",
  "src/panels/secret/index.tsx",
  "src/panels/tweaks/DiskSpaceAnalyzerDialog.tsx",
  "src/panels/tweaks/VmSandboxSection.tsx",
];

export default [
  {
    ignores: [
      "dist/**",
      "node_modules/**",
      "src-tauri/**",
      "cloudflare-license-worker/**",
      "ref/**",
      "tools/**",
      "assets/**",
      "**/*.config.{js,ts,mjs,cjs}",
      "**/vite.config.*",
      "eslint.config.js",
    ],
  },
  {
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: {
      parser: tsparser,
      parserOptions: {
        ecmaVersion: 2023,
        sourceType: "module",
        ecmaFeatures: { jsx: true },
      },
      globals: {
        window: "readonly",
        document: "readonly",
        navigator: "readonly",
        console: "readonly",
        setTimeout: "readonly",
        clearTimeout: "readonly",
        setInterval: "readonly",
        clearInterval: "readonly",
        fetch: "readonly",
        URL: "readonly",
      },
    },
    plugins: {
      "@typescript-eslint": tseslint,
      react,
      "react-hooks": reactHooks,
    },
    settings: {
      react: { version: "detect" },
    },
    rules: {
      // ── Security-critical (errors) ─────────────────────────────────
      "react/no-danger": "error",
      "no-eval": "error",
      "no-implied-eval": "error",
      "no-new-func": "error",

      // ── React hooks correctness ─────────────────────────────────────
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "warn",

      // ── Catch obvious bugs ──────────────────────────────────────────
      "no-debugger": "warn",
      "no-unreachable": "warn",
      "no-self-compare": "warn",
    },
  },
  // ── B5: array-index key guard on files already fixed ─────────────────────
  // Scoped as "error" so regressions in these files fail lint immediately.
  // Remaining files with pre-existing violations are not escalated yet;
  // promote to project-wide "warn" once those are cleaned up.
  {
    files: B5_FIXED_FILES,
    plugins: { react },
    rules: {
      "react/no-array-index-key": "error",
    },
  },
  // ── A6: IPC-layering guard ────────────────────────────────────────────────
  // Raw `invoke` from @tauri-apps/api/core must only be called inside
  // src/hooks/** (typed wrapper hooks, e.g. useArgus.ts, useBackend.ts).
  // Everywhere else should call a hook instead of hitting Tauri IPC directly,
  // so response shapes stay typed/centralized in one layer.
  //
  // ~65 pre-existing files still import it directly; LEGACY_RAW_INVOKE_FILES
  // grandfathers them so this lands as a forward-looking guard (new code +
  // the migrated Argus panels) without turning `bun run lint` red. This list
  // is a shrinking migration backlog, not a permanent exemption — remove a
  // file from it as it's migrated to a hook; never add to it.
  {
    files: ["src/**/*.{ts,tsx}"],
    ignores: ["src/hooks/**", ...LEGACY_RAW_INVOKE_FILES],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          paths: [
            {
              name: "@tauri-apps/api/core",
              message:
                "Raw Tauri invoke() is only allowed in src/hooks/**. Add or extend a typed hook (see src/hooks/useArgus.ts) instead of calling invoke() directly.",
            },
          ],
        },
      ],
    },
  },
];
