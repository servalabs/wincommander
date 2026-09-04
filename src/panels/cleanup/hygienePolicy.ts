import type { CleanupCategory } from "./cleanupCategories";

/**
 * Bulk hygiene is deliberately an allowlist rather than a severity/tier query.
 * Cleanup categories cover everything from disposable caches to credentials and
 * security evidence; metadata alone is not a safe authorization boundary.
 */
export const ROUTINE_HYGIENE_CATEGORY_IDS = [
  "dnsCache",
  "thumbnailDb",
  "spotlightCache",
  "fontCache",
  "legacyIconCache",
  "photosCache",
  "xboxCache",
  "branchCache",
  "p2pUpdateCache",
  "geolocationCache",
] as const;

export type RoutineHygieneCategoryId = typeof ROUTINE_HYGIENE_CATEGORY_IDS[number];

const ROUTINE_HYGIENE_ID_SET: ReadonlySet<string> = new Set(
  ROUTINE_HYGIENE_CATEGORY_IDS,
);

/**
 * Regression guardrails for categories that must never drift into the routine
 * bulk preset. Individual expert actions keep their existing confirmations and
 * authorization paths; this list only protects the guided preset.
 */
export const ROUTINE_HYGIENE_GUARDRAIL_IDS = {
  securityEvidence: [
    "eventLogs",
    "defenderHistory",
    "firewallLog",
    "usageTraceLogs",
    "servicingLogs",
    "deviceInstallLogs",
    "remoteAccessLogs",
    "gameLauncherLogs",
    "reliabilityHistory",
    "appLaunchHistory",
    "amcache",
    "execCache",
    "prefetchFiles",
    "ntfsJournals",
    "srumData",
    "eventTranscript",
  ],
  historiesAndIdentity: [
    "psHistory",
    "browserFootprints",
    "webCache",
    "sshState",
    "credentialManager",
    "passwordManagerCaches",
    "wlanProfiles",
    "vpnPhonebooks",
    "rdpHistory",
    "officeMru",
    "editorHistory",
    "gitActivity",
  ],
  dataAndRecovery: [
    "shadowCopies",
    "recycleBin",
    "wslData",
    "dockerDesktopData",
    "virtualMachineArtifacts",
    "developerCaches",
    "inactiveUserProtectionMetadata",
    "stickyNotes",
    "oneDriveMetadata",
    "communicationCaches",
    "cloudPlaceholders",
    "bitsQueue",
    "gameCaptures",
    "officeTempFiles",
  ],
} as const;

export interface HygieneCardSnapshot {
  count?: number;
  loading?: boolean;
  clearing?: boolean;
  error?: string;
}

export interface RoutineHygienePlan {
  categories: CleanupCategory[];
  ready: CleanupCategory[];
  clean: CleanupCategory[];
  unscanned: CleanupCategory[];
  busy: CleanupCategory[];
  failed: CleanupCategory[];
  missingCategoryIds: RoutineHygieneCategoryId[];
  totalFindings: number;
  previewComplete: boolean;
  canClear: boolean;
}

export function isRoutineHygieneCategory(categoryId: string): categoryId is RoutineHygieneCategoryId {
  return ROUTINE_HYGIENE_ID_SET.has(categoryId);
}

export function getRoutineHygieneCategories(
  categories: readonly CleanupCategory[],
): CleanupCategory[] {
  const byId = new Map(categories.map(category => [category.id, category]));
  return ROUTINE_HYGIENE_CATEGORY_IDS.flatMap(id => {
    const category = byId.get(id);
    return category ? [category] : [];
  });
}

export function buildRoutineHygienePlan(
  categories: readonly CleanupCategory[],
  snapshots: Readonly<Record<string, HygieneCardSnapshot | undefined>>,
): RoutineHygienePlan {
  const routineCategories = getRoutineHygieneCategories(categories);
  const foundIds = new Set(routineCategories.map(category => category.id));
  const missingCategoryIds = ROUTINE_HYGIENE_CATEGORY_IDS.filter(id => !foundIds.has(id));

  const busy = routineCategories.filter(category => {
    const state = snapshots[category.id];
    return !!state?.loading || !!state?.clearing;
  });
  const failed = routineCategories.filter(category => {
    const error = snapshots[category.id]?.error;
    return typeof error === "string" && error.trim().length > 0;
  });
  const unscanned = routineCategories.filter(category => {
    const count = snapshots[category.id]?.count;
    return typeof count !== "number" || count < 0;
  });
  const ready = routineCategories.filter(category => {
    const state = snapshots[category.id];
    return !state?.loading && !state?.clearing && typeof state?.count === "number" && state.count > 0;
  });
  const clean = routineCategories.filter(category => snapshots[category.id]?.count === 0);
  const totalFindings = ready.reduce(
    (total, category) => total + Math.max(0, snapshots[category.id]?.count ?? 0),
    0,
  );
  const previewComplete = missingCategoryIds.length === 0
    && unscanned.length === 0
    && busy.length === 0
    && failed.length === 0;

  return {
    categories: routineCategories,
    ready,
    clean,
    unscanned,
    busy,
    failed,
    missingCategoryIds,
    totalFindings,
    previewComplete,
    canClear: previewComplete && ready.length > 0,
  };
}
