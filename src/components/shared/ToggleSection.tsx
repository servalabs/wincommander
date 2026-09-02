import { useState, useCallback, useMemo } from "react";
import UniversalToggle from "./UniversalToggle";
import LockedToggle from "./LockedToggle";
import SectionCard from "./SectionCard";
import ConflictToggleDialog from "./ConflictToggleDialog";
import PinEntryDialog from "./PinEntryDialog";
import { executeBackendCommand } from "../../hooks/useBackend";
import { showError } from "../../utils/toast";
import { getByPath, getToggleVisibility, buildToggleCommandParams } from "../../types/toggles";
import { useSettingsQuery } from "../../hooks/queries/useSettingsQuery";
import { useAppState } from "../../context/AppContext";
import useVisibility from "../../hooks/useVisibility";
import useEntitlements from "../../hooks/useEntitlements";
import { resolveToggleText } from "../../types/toggles";
import { ALL_TOGGLES } from "../../registry";
import { resolveToggleIcon } from "../../registry/toggleIcons";
import { useManagedPolicy, isToggleLocked } from "../../hooks/useManagedPolicy";
import { isPrivilegedWriteBlocked, MACHINE_SCOPE_ELEVATION_MESSAGE } from "../../lib/machineScopeElevation";
import type { ExperienceLevel } from "../../types/settings";
import type { ToggleDef, SectionDef } from "../../types/toggles";

interface ToggleSectionProps {
  /** The section header definition (title, icon, columns) */
  section: SectionDef;
  /** All toggle definitions — will be filtered to this section automatically */
  toggles: ToggleDef[];
  /** Called AFTER a toggle's backend command completes successfully. */
  onToggled?: (toggle: ToggleDef, checked: boolean) => void;
  /** Resolve whether a toggle should be disabled (un-clickable). */
  resolveDisabled?: (toggle: ToggleDef) => boolean;
  /** Extra CSS class(es) for the inner grid div. */
  gridClassName?: string;
  /** If true, renders ONLY the grid — no wrapping SectionCard. */
  bare?: boolean;
  /** Optional search query for filtering toggles. */
  searchQuery?: string;
  /** Content to display on the right side of the section header */
  headerRight?: React.ReactNode;
  /** Supplemental content that belongs in the same card below the toggle grid. */
  footer?: React.ReactNode;
  /** Accordion: Is this section collapsible? */
  collapsible?: boolean;
  /** Accordion: Is this section currently open? */
  isOpen?: boolean;
  /** Accordion: Callback when header is clicked */
  onToggle?: () => void;
  /** Custom conflicts for non-registry controls that still share state. */
  getExternalActiveConflicts?: (toggle: ToggleDef) => ExternalToggleConflict[];
}

export interface ExternalToggleConflict {
  id: string;
  label: string;
  disable?: () => Promise<void>;
}

export function orderTogglesForDisplay(
  toggles: readonly ToggleDef[],
  _isChecked: (toggle: ToggleDef) => boolean,
): ToggleDef[] {
  return [...toggles];
}

export default function ToggleSection({
  section,
  toggles,
  onToggled,
  resolveDisabled,
  gridClassName,
  bare,
  searchQuery = "",
  headerRight,
  footer,
  collapsible,
  isOpen,
  onToggle,
  getExternalActiveConflicts,
}: ToggleSectionProps) {
  // ── Environment Hook (Fast SSOT — ensures instant label updates) ──
  const visibility = useVisibility();
  const currentLevel: ExperienceLevel = visibility.density === "expert" ? "advanced" : "standard";

  // ── Entitlements (paid features get LockedToggle when no entitlement) ──
  const { canUse } = useEntitlements();

  // ── Settings from React Query (Database Ground Truth) ──────────────
  const { data: appSettings } = useSettingsQuery();
  const { refreshSettings, systemInfo } = useAppState();

  // ── Admin policy lock: settings whose ideal path is in policy.lockedPaths
  //    are pushed by the org's master config and cannot be changed locally.
  const lockedPaths = useMemo(
    () => appSettings?.policy?.lockedPaths ?? [],
    [appSettings?.policy?.lockedPaths],
  );
  const isLocked = useCallback(
    (path: string) => lockedPaths.some((p) => path.startsWith(p)),
    [lockedPaths]
  );

  // ── F9 phase-2: GPO/MDM managed-policy toggle locking ────────────────
  // Fetched ONCE at the section level (single IPC per 60 s cadence).
  // isToggleLocked() is a pure local check — zero N+1 risk.
  // When no policy is present (the common case), managedPolicy.values is {}
  // and isPolicyLocked() always returns false — zero behavior change.
  const managedPolicy = useManagedPolicy();
  const isPolicyLocked = useCallback(
    (toggleId: string) => isToggleLocked(managedPolicy.values, toggleId),
    [managedPolicy.values],
  );

  // ── Pending state: toggles currently being applied ───────────────
  const [pendingMap, setPendingMap] = useState<Record<string, boolean>>({});

  // Filter toggles belonging to THIS section AND matching search query
  const visibleSectionToggles = toggles.filter((t) => {
    const isInSection = t.section === section.id;
    if (!isInSection) return false;
    // KT: selected capability bundles are needed to classify legacy
    // minExperience:"standard" rows. Without this, safeguards-only rows
    // can leak into Guided profiles that never asked for safeguards.
    if (!visibility.isVisible(getToggleVisibility(t, visibility.profiles))) return false;
    if (!searchQuery.trim()) return true;

    const q = searchQuery.toLowerCase().trim();
    const wording = resolveToggleText(t, currentLevel || "simple");
    return (
      wording.label.toLowerCase().includes(q) ||
      wording.description.toLowerCase().includes(q) ||
      t.label.toLowerCase().includes(q) ||
      t.description.toLowerCase().includes(q) ||
      (t.keywords && t.keywords.some((k: string) => k.toLowerCase().includes(q)))
    );
  });

  /**
   * Get the real "checked" value for a toggle from appSettings.
   */
  const getChecked = useCallback(
    (toggle: ToggleDef): boolean => {
      if (appSettings) {
        const raw = getByPath(appSettings, toggle.currentPath);
        if (toggle.checkedWhen !== undefined) return raw === toggle.checkedWhen;
        return Boolean(raw);
      }
      return false;
    },
    [appSettings]
  );

  const sectionToggles = useMemo(
    () => orderTogglesForDisplay(visibleSectionToggles, getChecked),
    [visibleSectionToggles, getChecked],
  );

  /**
   * Get the level-appropriate label and description for a toggle.
   */
  const getWording = useCallback(
    (toggle: ToggleDef): { label: string; description: string } => {
      return resolveToggleText(toggle, currentLevel || "simple");
    },
    [currentLevel]
  );

  /**
   * Returns the toggle defs that conflict with the given toggle (checked
   * symmetrically — either side of the conflictsWith declaration).
   */
  const getConflictingToggles = useCallback(
    (toggle: ToggleDef): ToggleDef[] => {
      return ALL_TOGGLES.filter((t) => {
        if (t.id === toggle.id) return false;
        return toggle.conflictsWith?.includes(t.id) || t.conflictsWith?.includes(toggle.id);
      });
    },
    []
  );

  /**
   * Dispatch a single toggle's backend command and refresh settings.
   * Returns true iff the backend command actually succeeded — callers that
   * chain follow-up side effects (e.g. disabling a conflicting toggle) MUST
   * check this before proceeding, since a thrown/failed apply must not be
   * treated as if it went through.
   */
  const applyToggle = useCallback(
    async (toggle: ToggleDef, checked: boolean, extraParams?: Record<string, string | number | boolean>): Promise<boolean> => {
      if (isPrivilegedWriteBlocked(toggle.needsAdmin, systemInfo?.isAdmin)) {
        showError(MACHINE_SCOPE_ELEVATION_MESSAGE);
        return false;
      }
      setPendingMap((prev) => ({ ...prev, [toggle.id]: true }));

      try {
        let result;
        if (toggle.capabilityKey) {
          result = await executeBackendCommand("Set-AppCapabilityAccess", {
            Capability: toggle.capabilityKey,
            Access: checked ? "Deny" : "Allow",
          });
        } else {
          const cmd = checked ? toggle.enableCmd : toggle.disableCmd;
          result = await executeBackendCommand(cmd, buildToggleCommandParams(toggle, checked, extraParams));
        }

        if (!result.success) {
          showError(result.error || `Failed to ${checked ? "enable" : "disable"} ${toggle.label}`);
          return false;
        }

        // refreshSettings() already re-reads settings.json, updates appSettings, and
        // the AppContext mirrors it into the React Query cache — the extra
        // refetchQueries was a redundant second IPC that compounded the freeze on
        // rapid toggling.
        await refreshSettings();
        onToggled?.(toggle, checked);
        return true;
      } catch (err) {
        showError(err instanceof Error ? err.message : String(err));
        return false;
      } finally {
        setPendingMap((prev) => {
          const next = { ...prev };
          delete next[toggle.id];
          return next;
        });
      }
    },
    [onToggled, refreshSettings, systemInfo?.isAdmin]
  );

  // ── Conflict confirmation: pending toggle awaiting user decision ────
  const [pendingConflict, setPendingConflict] = useState<{
    toggle: ToggleDef;
    activeConflicts: ToggleDef[];
    externalConflicts: ExternalToggleConflict[];
  } | null>(null);

  /**
   * Handle a toggle flip. When enabling a toggle whose conflictsWith
   * includes a currently-ACTIVE toggle, prompt for confirmation before
   * applying anything — confirming enables this toggle and disables the
   * conflicting one(s); cancelling leaves everything unchanged.
   */
  const handleToggle = useCallback(
    async (toggle: ToggleDef, checked: boolean) => {
      if (checked) {
        // PIN-gated toggles (e.g. bitlockerTpmPinEnforce) collect the PIN
        // via a dialog before dispatching the enable command — none of
        // these currently declare conflictsWith, so the conflict-check
        // path below is skipped for them.
        if (toggle.requiresPinOnEnable) {
          setPendingPinToggle(toggle);
          return;
        }
        const activeConflicts = getConflictingToggles(toggle).filter((t) => getChecked(t));
        const externalConflicts = getExternalActiveConflicts?.(toggle) ?? [];
        if (activeConflicts.length > 0 || externalConflicts.length > 0) {
          setPendingConflict({ toggle, activeConflicts, externalConflicts });
          return;
        }
      }
      await applyToggle(toggle, checked);
    },
    [applyToggle, getConflictingToggles, getChecked, getExternalActiveConflicts]
  );

  // ── PIN entry: toggle awaiting a PIN before its enable command fires ──
  const [pendingPinToggle, setPendingPinToggle] = useState<ToggleDef | null>(null);

  const handleConfirmPin = useCallback(
    async (pin: string) => {
      if (!pendingPinToggle) return;
      const ok = await applyToggle(pendingPinToggle, true, { Pin: pin });
      if (ok) setPendingPinToggle(null);
      // On failure, leave the dialog open — applyToggle already surfaced
      // the backend error via toast, and the user can correct the PIN.
    },
    [pendingPinToggle, applyToggle]
  );

  const handleCancelPin = useCallback(() => {
    setPendingPinToggle(null);
  }, []);

  const handleConfirmConflict = useCallback(async () => {
    if (!pendingConflict) return;
    const { toggle, activeConflicts, externalConflicts } = pendingConflict;
    setPendingConflict(null);

    // Mark the conflicts as pending immediately so a user can't manually
    // flip one mid-sequence while we're still deciding whether to disable it.
    setPendingMap((prev) => {
      const next = { ...prev };
      for (const conflict of activeConflicts) next[conflict.id] = true;
      return next;
    });

    const enabled = await applyToggle(toggle, true);
    if (!enabled) {
      // Enabling the target failed — leave the conflicting toggles untouched
      // rather than disabling them anyway (which would leave both OFF).
      setPendingMap((prev) => {
        const next = { ...prev };
        for (const conflict of activeConflicts) delete next[conflict.id];
        return next;
      });
      return;
    }
    for (const conflict of activeConflicts) {
      await applyToggle(conflict, false);
    }
    for (const conflict of externalConflicts) {
      await conflict.disable?.();
    }
  }, [pendingConflict, applyToggle]);

  const handleCancelConflict = useCallback(() => {
    setPendingConflict(null);
  }, []);

  // If this section has no toggles, don't render anything
  if (sectionToggles.length === 0) return null;

  const grid = (
    <div
      className={`grid gap-4 grid-cols-${section.columns ?? 3}${gridClassName ? ` ${gridClassName}` : ""}`}
    >
      {sectionToggles.map((toggle) => {
        const wording = getWording(toggle);

        // Paid feature without entitlement → render the locked variant.
        // Same row layout, no switch, click opens the Paywall.
        if (!canUse(toggle.tier)) {
          return (
            <LockedToggle
              key={toggle.id}
              label={wording.label}
              description={wording.description}
              icon={resolveToggleIcon(toggle)}
              domain={toggle.domain}
              size="compact"
            />
          );
        }

        const orgLocked = isLocked(toggle.settingsPath) || isPolicyLocked(toggle.id);
        const needsElevation = isPrivilegedWriteBlocked(toggle.needsAdmin, systemInfo?.isAdmin);

        return (
          <div key={toggle.id}>
            <UniversalToggle
              label={wording.label}
              description={needsElevation ? `${wording.description} Requires an administrator.` : wording.description}
              checked={getChecked(toggle)}
              onChange={(checked) => handleToggle(toggle, checked)}
              loading={pendingMap[toggle.id]}
              disabled={
                pendingMap[toggle.id] ||
                orgLocked ||
                needsElevation ||
                (resolveDisabled ? resolveDisabled(toggle) : false)
              }
              managedByOrg={orgLocked}
              riskLevel={
                toggle.irreversible || toggle.reducesSecurity || toggle.defenderFlagged
                  ? "high"
                  : "low"
              }
              requiresRestart={toggle.requiresRestart}
              icon={resolveToggleIcon(toggle)}
              severity={toggle.severity}
              domain={toggle.domain as any}
              size="compact"
              riskFlags={{
                needsAdmin: toggle.needsAdmin,
                irreversible: toggle.irreversible,
                reducesSecurity: toggle.reducesSecurity,
                defenderFlagged: toggle.defenderFlagged,
              }}
            />
            {needsElevation && <p role="alert" className="mt-1 text-xs text-[var(--warn)]">{MACHINE_SCOPE_ELEVATION_MESSAGE}</p>}
          </div>
        );
      })}
    </div>
  );

  const conflictDialog = pendingConflict && (
    <ConflictToggleDialog
      isOpen={true}
      toggleLabel={pendingConflict.toggle.label}
      conflictingLabels={[
        ...pendingConflict.activeConflicts.map((t) => t.label),
        ...pendingConflict.externalConflicts.map((t) => t.label),
      ]}
      onCancel={handleCancelConflict}
      onConfirm={handleConfirmConflict}
    />
  );

  const pinDialog = pendingPinToggle && (
    <PinEntryDialog
      isOpen={true}
      toggleLabel={pendingPinToggle.label}
      description={pendingPinToggle.description}
      pinPattern={pendingPinToggle.pinPattern}
      onCancel={handleCancelPin}
      onConfirm={handleConfirmPin}
    />
  );

  // bare mode: just the grid, no wrapping SectionCard
  if (bare) {
    return (
      <>
        {grid}
        {conflictDialog}
        {pinDialog}
      </>
    );
  }

  return (
    <>
      <SectionCard
        title={section.title}
        icon={section.icon}
        headerRight={headerRight}
        collapsible={collapsible}
        isOpen={isOpen}
        onToggle={onToggle}
      >
        {grid}
        {footer && <div className="mt-4 border-t border-[var(--border)] pt-4">{footer}</div>}
      </SectionCard>
      {conflictDialog}
      {pinDialog}
    </>
  );
}
