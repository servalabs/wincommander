// src/panels/privacy/LockdownConfigSection.tsx
//
// Inline configuration block for the universal lockdown cascade.
// Reads / writes `privacy.selfDestruct` — the same settings block the
// Rust orchestrator (`full_lockdown` in
// commander-free/src/backend.rs) reads at fire time. Every lockdown
// trigger (sidebar button, Ctrl+Shift+Q, lockdown
// words) honours these choices.
//
// Renders WITHOUT its own SectionCard — meant to be embedded inside a
// parent SectionCard. Now lives in the Secret Settings panel.
//
// Layout:
//   - Status strip across the top (X/Y enabled, Reset)
//   - Collapsible group blocks: chevron + icon + title + count badge
//     + bulk Enable/Disable. Default-collapsed because the panel is
//     already content-dense; users opt in to a group to customise.
//   - Compact rows: checkbox + monospace label + plain-English
//     description from DESTRUCT_STEP_DESCRIPTIONS
//   - Privacy Clean group is danger-styled (red border + tinted body)
//     because each step takes minutes-hours and is irreversible
//   - "On Completion" group bundles the include_app step with the
//     three global flags (deactivate licence / shutdown / skip
//     browsers) using the same row component for a consistent rhythm
//
// Step list comes from src/types/lockdownSteps.ts (mirror of the Rust
// DESTRUCT_STEPS slice). Stable IDs are the keys in
// privacy.selfDestruct.steps. Sparse override map: writing the
// step's defaultEnabled value DELETES the override so future default
// changes flow through.

import { Button, Checkbox, Icon, Tooltip } from "@/components/ui/bp";
import type { IconName } from "@/components/ui/bp";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import CreateWipeUsbDialog from "./CreateWipeUsbDialog";
import VerifyUsbBootDialog from "./VerifyUsbBootDialog";
import RemoveUsersSection from "./RemoveUsersSection";
import CryptoEraseTargetsSection from "./CryptoEraseTargetsSection";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import TierGate from "../../components/shared/TierGate";
import { showError, showSuccess } from "../../utils/toast";
import {
  DESTRUCT_STEPS,
  DESTRUCT_GROUP_LABELS,
  DESTRUCT_STEP_DESCRIPTIONS,
  type DestructGroup,
  type DestructStepDef,
  isStepEnabled,
} from "../../types/lockdownSteps";
import type { SelfDestructSettings } from "../../types/settings";
import { useAppState } from "../../context/AppContext";
import { getDisplayBranding } from "../../lib/branding";
import "./LockdownConfigSection.css";

interface Props {
  /** Passed by call sites that manage their own settings context; falls back to
   *  useAppState when omitted so the component requires no props. */
  searchQuery?: string;
  config?: SelfDestructSettings;
  onPatch?: (patch: SelfDestructSettings) => void;
}

const ORDERED_GROUPS: DestructGroup[] = [
  "systemCleaner",
  "privacyTraces",
  "deepDfir",
  "privacyClean",
];

const GROUP_ICONS: Record<DestructGroup, IconName> = {
  systemCleaner: "clean",
  privacyTraces: "shield",
  deepDfir: "search-template",
  privacyClean: "warning-sign",
  appRemoval: "trash",
};

export default function LockdownConfigSection(props: Props = {}) {
  const { appSettings, patchAppSettings } = useAppState();
  const { productName } = getDisplayBranding(appSettings);
  const ramdiskAutostart = appSettings?.app?.vault?.ramdiskAutostart;
  const skipRamdiskAfterLockdown = !!ramdiskAutostart?.skipAfterLockdown;
  const internalConfig = appSettings?.ideal?.privacy?.selfDestruct;
  const internalOnPatch = useCallback(
    (patch: SelfDestructSettings) =>
      patchAppSettings({ ideal: { privacy: { selfDestruct: patch } } } as any).catch(() => {}),
    [patchAppSettings],
  );
  const config = props.config !== undefined ? props.config : internalConfig;
  const onPatch = props.onPatch ?? internalOnPatch;
  const searchQuery = props.searchQuery ?? "";

  const steps = useMemo(() => config?.steps ?? {}, [config?.steps]);

  // Search filter — when the user is searching the panel, only show
  // the section if at least one step's label / description matches,
  // and only render those rows. Keeps the panel-search experience
  // consistent with the rest of the privacy panel.
  const search = searchQuery.trim().toLowerCase();
  const matches = useCallback(
    (def: DestructStepDef): boolean => {
      if (!search) return true;
      if (def.label.toLowerCase().includes(search)) return true;
      const desc = DESTRUCT_STEP_DESCRIPTIONS[def.id]?.toLowerCase() ?? "";
      return desc.includes(search);
    },
    [search],
  );

  const stepsByGroup = useCallback(
    (group: DestructGroup): DestructStepDef[] =>
      DESTRUCT_STEPS.filter((s) => s.group === group && matches(s)),
    [matches],
  );

  const enabledInGroup = useCallback(
    (group: DestructGroup): { enabled: number; total: number } => {
      const all = DESTRUCT_STEPS.filter((s) => s.group === group);
      const enabled = all.filter((s) => isStepEnabled(s, steps)).length;
      return { enabled, total: all.length };
    },
    [steps],
  );

  // Always write an explicit boolean. We previously tried to be
  // clever and DELETE the override when toggling back to the default
  // (sparse map), but the patch endpoint uses deep-merge — which has
  // no way to express "delete a key". A patch like `{ steps: {} }`
  // would leave the on-disk override in place, so the checkbox
  // appeared to re-tick but the next reload reverted. Always-explicit
  // writes are correct, and the resulting ~35-key boolean map adds
  // <1KB to settings.json.
  // Send only the changed key(s), not the full `steps` snapshot: the settings
  // patch endpoint deep-merges nested objects key-by-key, so a minimal patch
  // is sufficient — and unlike a full spread of this render's `steps` closure,
  // it can't clobber a sibling step whose own recent write is still in flight
  // (queued ahead of this one) and hasn't been reflected back into this
  // component's `steps` yet.
  const writeStep = useCallback(
    (id: string, enabled: boolean) => {
      onPatch({ steps: { [id]: enabled } });
    },
    [onPatch],
  );

  const setAllInGroup = useCallback(
    (group: DestructGroup, enabled: boolean) => {
      const next: Record<string, boolean> = {};
      for (const def of DESTRUCT_STEPS.filter((s) => s.group === group)) {
        next[def.id] = enabled;
      }
      onPatch({ steps: next });
    },
    [onPatch],
  );

  // Reset writes the explicit defaults for every step (rather than
  // sending an empty object). Same reason as writeStep: deep-merge
  // can't delete previous overrides; we have to overwrite them with
  // the documented defaults.
  const resetAll = useCallback(() => {
    const next: Record<string, boolean> = {};
    for (const def of DESTRUCT_STEPS) {
      next[def.id] = def.defaultEnabled;
    }
    onPatch({ steps: next });
  }, [onPatch]);

  const setGlobal = useCallback(
    (key: keyof SelfDestructSettings, value: boolean) => {
      onPatch({ [key]: value } as SelfDestructSettings);
    },
    [onPatch],
  );

  // ── User-folder shred list ─────────────────────────────────────────
  // User-supplied folder paths that get securely shredded (Invoke-7Erase,
  // single durable RNG-overwrite pass by default, user-configurable up to 7)
  // BEFORE the Rust cascade fires (RightSidebar.fireSelfDestruct). Runs first so user
  // data is destroyed even if a later step fails / machine is yanked
  // mid-cascade. Settings field: ideal.privacy.selfDestruct.shredFolders.
  const shredFolders = useMemo(() => config?.shredFolders ?? [], [config?.shredFolders]);

  // Local usernames selected for removal by the remove_users destruct step.
  // See RemoveUsersSection for the picker UI; settings field is
  // ideal.privacy.selfDestruct.usersToRemove.
  const usersToRemove = useMemo(() => config?.usersToRemove ?? [], [config?.usersToRemove]);

  // Target lists for the bitlocker_erase / veracrypt_header_destroy destruct
  // steps. See CryptoEraseTargetsSection for the picker UI; settings fields
  // are ideal.privacy.selfDestruct.cryptoEraseBitlockerDrives /
  // .cryptoEraseVeracryptPaths.
  const cryptoEraseBitlockerDrives = useMemo(
    () => config?.cryptoEraseBitlockerDrives ?? [],
    [config?.cryptoEraseBitlockerDrives],
  );
  const cryptoEraseVeracryptPaths = useMemo(
    () => config?.cryptoEraseVeracryptPaths ?? [],
    [config?.cryptoEraseVeracryptPaths],
  );

  const addShredFolder = useCallback(async () => {
    try {
      const picked = await openDialog({
        multiple: false,
        directory: true,
        title: "Pick a folder to delete on lockdown",
      });
      if (typeof picked !== "string") return;
      if (shredFolders.includes(picked)) {
        showError("Folder is already in the delete list.");
        return;
      }
      onPatch({ shredFolders: [...shredFolders, picked] });
      showSuccess(`Will delete ${picked.split(/[/\\]/).slice(-2).join("/")} on lockdown.`);
    } catch (err) {
      showError(`Couldn't add folder: ${err}`);
    }
  }, [shredFolders, onPatch]);

  const removeShredFolder = useCallback(
    (path: string) => {
      onPatch({ shredFolders: shredFolders.filter((p) => p !== path) });
    },
    [shredFolders, onPatch],
  );

  const totals = useMemo(() => {
    const enabled = DESTRUCT_STEPS.filter((d) => isStepEnabled(d, steps)).length;
    return { enabled, total: DESTRUCT_STEPS.length };
  }, [steps]);

  const includeAppDef = DESTRUCT_STEPS.find((s) => s.id === "include_app")!;
  const includeApp = isStepEnabled(includeAppDef, steps);
  const shutdownSystem = config?.shutdownSystem ?? true;

  // Search-mode quick exit: hide the whole section when the search
  // term matches nothing in the registry. Keeps the panel uncluttered
  // when the user is hunting for something else.
  if (search) {
    const anyMatch = DESTRUCT_STEPS.some(matches);
    if (!anyMatch) return null;
  }

  return (
    <TierGate tier="paid" featureLabel="Lockdown Configuration">
      <div className="sd-config-block">
        <div className="sd-config-status">
          <div>
            <span className="sd-config-status-count">
              {totals.enabled} / {totals.total}
            </span>
            <span className="sd-config-status-meta">
              steps enabled
              {shredFolders.length > 0 && ` · ${shredFolders.length} folder${shredFolders.length === 1 ? "" : "s"} to delete on lockdown`}
              {usersToRemove.length > 0 && ` · ${usersToRemove.length} user${usersToRemove.length === 1 ? "" : "s"} to remove on lockdown`}
              {includeApp ? " · app will be uninstalled" : " · app stays installed"}
              {shutdownSystem ? " · shutdown after" : ""}
            </span>
          </div>
          <div className="sd-config-status-actions">
            <Tooltip content="Restore every step to its registry default" position="left">
              <Button
                className="sd-bulk-btn"
                minimal
                small
                onClick={resetAll}
                icon="reset"
                text="Reset"
              />
            </Tooltip>
          </div>
        </div>

        {/* User-defined folder shred list. Runs BEFORE the Rust cascade
            (RightSidebar.fireSelfDestruct) so user data is destroyed
            first, regardless of which other steps are enabled. Each
            folder goes through the secure shredder command (single
            durable RNG-overwrite pass + GUID rename) — irreversible on
            HDD, best-effort on SSD due to wear-levelling. */}
        {/* Folders-to-delete and users-to-remove pickers side by side — both are
            compact target-picker cards, so a 2-column row uses panel width
            better than stacking them full-width. Collapses to 1 column below
            the same 720px breakpoint sd-rows-grid.is-two-col already uses. */}
        <div className="sd-targets-row">
          <div className="sd-shred-folders">
            <div className="sd-shred-folders-header">
              <Icon icon="folder-close" size={14} className="sd-shred-folders-icon" />
              <div className="sd-shred-folders-text">
                <div className="sd-shred-folders-label">Folders to Delete on Lockdown</div>
              </div>
              <Button
                className="sd-bulk-btn"
                minimal
                small
                icon="plus"
                text="Add folder"
                onClick={addShredFolder}
              />
            </div>
            {shredFolders.length > 0 && (
              <div className="sd-shred-folders-list">
                {shredFolders.map((p) => (
                  <div key={p} className="sd-shred-folder-row">
                    <Icon icon="folder-close" size={12} className="sd-shred-folder-icon" />
                    <span className="sd-shred-folder-path" title={p}>{p}</span>
                    <Button
                      minimal
                      small
                      icon="cross"
                      onClick={() => removeShredFolder(p)}
                      title="Remove from delete list"
                    />
                  </div>
                ))}
              </div>
            )}
          </div>

          {/* Local accounts pre-selected for the remove_users destruct step.
              Pure prop-driven renderer — see RemoveUsersSection.tsx. */}
          <RemoveUsersSection usersToRemove={usersToRemove} onPatch={onPatch} />
        </div>

        {/* Target pickers for the bitlocker_erase / veracrypt_header_destroy
            destruct steps. Pure prop-driven renderer — see
            CryptoEraseTargetsSection.tsx. */}
        <CryptoEraseTargetsSection
          bitlockerDrives={cryptoEraseBitlockerDrives}
          veracryptPaths={cryptoEraseVeracryptPaths}
          onPatch={onPatch}
        />

        {/* Erase-target groups as columns in ONE card:
            Privacy Traces spans 2 cols (with an inner 2-col row grid),
            Deep Trace Cleaner = col 3, Privacy Deep Clean = col 4.
            "On Completion" is rendered as a toggles row BELOW this card. */}
        <div className="sd-columns-card">
          {ORDERED_GROUPS.map((group) => {
            const items = stepsByGroup(group);
            if (items.length === 0) return null;
            const counts = enabledInGroup(group);
            const isDanger = group === "privacyClean";
            const isWide = group === "privacyTraces";
            const allOn = counts.enabled === counts.total;
            const allOff = counts.enabled === 0;

            return (
              <div
                key={group}
                className={`sd-group ${isDanger ? "is-danger" : ""} ${isWide ? "is-wide" : ""}`}
              >
                <div className="sd-group-header is-static">
                  <Icon icon={GROUP_ICONS[group]} size={14} className="sd-group-icon" />
                  <span className="sd-group-title">{DESTRUCT_GROUP_LABELS[group]}</span>
                  <span className="sd-group-count">
                    {counts.enabled}/{counts.total}
                  </span>
                  <div className="sd-group-bulk">
                    <Tooltip content="Enable every step in this group" position="top">
                      <Button
                        className="sd-bulk-btn"
                        minimal
                        small
                        disabled={allOn}
                        onClick={() => setAllInGroup(group, true)}
                        text="All"
                      />
                    </Tooltip>
                    <Tooltip content="Disable every step in this group" position="top">
                      <Button
                        className="sd-bulk-btn"
                        minimal
                        small
                        disabled={allOff}
                        onClick={() => setAllInGroup(group, false)}
                        text="None"
                      />
                    </Tooltip>
                  </div>
                </div>

                <div className="sd-group-body">
                  <div className="sd-rows-grid">
                    {items.map((def) => (
                      <StepRow
                        key={def.id}
                        def={def}
                        checked={isStepEnabled(def, steps)}
                        onToggle={writeStep}
                      />
                    ))}
                  </div>
                </div>
              </div>
            );
          })}
        </div>

        {!search && (
          <div className="sd-completion-row">
            <span className="sd-completion-label">On Completion</span>
            <div className="sd-completion-toggles">
              <CompletionToggle
                label={`Uninstall ${productName}`}
                checked={includeApp}
                onToggle={(v) => writeStep("include_app", v)}
              />
              <CompletionToggle
                label="Deactivate licence"
                checked={config?.deactivateLicenseFirst ?? false}
                onToggle={(v) => setGlobal("deactivateLicenseFirst", v)}
              />
              <CompletionToggle
                label="Graceful Windows shutdown"
                checked={shutdownSystem}
                onToggle={(v) => setGlobal("shutdownSystem", v)}
              />
              <CompletionToggle
                label="Disable RAM disk autostart"
                checked={skipRamdiskAfterLockdown}
                onToggle={(v) =>
                  patchAppSettings({ app: { vault: { ramdiskAutostart: { ...(ramdiskAutostart ?? {}), skipAfterLockdown: v } } } } as any).catch(() => {})
                }
              />
            </div>
          </div>
        )}

        {/* ── F6 Reboot-to-USB Wipe arming toggle ──────────────────────────────
            Requires self-destruct to be enabled. Advanced users only.
            Enabling this is irreversible per-session and requires a 3-second
            countdown confirm so it cannot be accidentally armed.
            Writes `selfDestruct.rebootToUsbEnabled`.
            Decoy session write-gate is enforced server-side (Rust write_settings). */}
        {!search && (
          <RebootToUsbArming
            armed={config?.rebootToUsbEnabled ?? false}
            selfDestructEnabled={config?.enabled ?? false}
            onArm={(v) => setGlobal("rebootToUsbEnabled", v)}
          />
        )}

      </div>
    </TierGate>
  );
}

// Inline checkbox used by the "On Completion" row beneath the columns card.
function CompletionToggle({
  label,
  checked,
  onToggle,
}: {
  label: string;
  checked: boolean;
  onToggle: (v: boolean) => void;
}) {
  return (
    <Checkbox
      className="sd-completion-toggle"
      checked={checked}
      onChange={() => onToggle(!checked)}
      label={label}
    />
  );
}

// ── F6 Reboot-to-USB Wipe arming toggle ──────────────────────────
//
// Separate component so its countdown state is isolated and never leaks
// into the parent re-render cycle.
//
// Design rules (spec §6, §12):
//   - Disabled unless `selfDestructEnabled` is true.
//   - Enabling: shows a 3-second countdown. User must wait. Irreversible warning.
//   - Disabling: immediate (un-arming is always safe).
//   - minExperience: "advanced" — rendered inside a collapsed details block
//     so casual users never accidentally encounter it.

function RebootToUsbArming({
  armed,
  selfDestructEnabled,
  onArm,
}: {
  armed: boolean;
  selfDestructEnabled: boolean;
  onArm: (v: boolean) => void;
}) {
  const [countdown, setCountdown] = useState<number | null>(null);
  const [wizardOpen, setWizardOpen] = useState(false);
  const [verifyBootOpen, setVerifyBootOpen] = useState(false);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Clear countdown timer on unmount.
  useEffect(() => () => {
    if (timerRef.current) clearInterval(timerRef.current);
  }, []);

  const startArm = useCallback(() => {
    if (countdown !== null) return; // already counting
    setCountdown(3);
    timerRef.current = setInterval(() => {
      setCountdown((prev) => {
        if (prev === null || prev <= 1) {
          clearInterval(timerRef.current!);
          timerRef.current = null;
          // Fire arm after state update to avoid stale closure.
          setTimeout(() => onArm(true), 0);
          return null;
        }
        return prev - 1;
      });
    }, 1000);
  }, [countdown, onArm]);

  const cancelArm = useCallback(() => {
    if (timerRef.current) {
      clearInterval(timerRef.current);
      timerRef.current = null;
    }
    setCountdown(null);
  }, []);

  const handleToggle = useCallback(() => {
    if (armed) {
      // Disarming is always immediate — no countdown.
      cancelArm();
      onArm(false);
    } else {
      startArm();
    }
  }, [armed, cancelArm, onArm, startArm]);

  const isDisabled = !selfDestructEnabled;

  return (
    <section className="sd-f6-wipe-card" aria-label="Reboot-to-USB wipe">
      <div className="sd-f6-wipe-card__header">
        <div className="sd-f6-wipe-card__title">
          <Icon icon="warning-sign" size={16} />
          <div>
            <div className="sd-f6-wipe-card__eyebrow">Advanced recovery action</div>
            <h4>Reboot-to-USB Wipe</h4>
          </div>
        </div>
        <span className={`sd-f6-wipe-card__status${armed ? " is-armed" : ""}`}>
          {countdown !== null ? `Arming in ${countdown}s` : armed ? "Armed" : "Not armed"}
        </span>
      </div>

      <p className="sd-f6-wipe-card__warning">
        A <code>reboot_usb</code> distress phrase runs crypto-erase first, then reboots into a provisioned USB sanitiser. <strong>This is irreversible.</strong> If no wipe USB is attached, only stage one runs.
      </p>

      {isDisabled && (
        <p className="sd-f6-wipe-card__gated">Enable Self-Destruct to arm this action or prepare a wipe USB.</p>
      )}

      <div className="sd-f6-wipe-card__actions">
        <Checkbox
          disabled={isDisabled}
          checked={armed}
          onChange={countdown !== null ? undefined : handleToggle}
          label={countdown !== null ? "Arming — click Cancel to stop" : armed ? "Reboot-to-USB wipe is armed" : "Arm reboot-to-USB wipe"}
        />
        {countdown !== null && (
          <Button small intent="danger" text="Cancel" onClick={cancelArm} />
        )}
        <div className="sd-f6-wipe-card__usb-actions">
          <Button
            small
            icon="floppy-disk"
            text="Create Wipe USB…"
            disabled={isDisabled}
            onClick={() => setWizardOpen(true)}
          />
          <Button
            small
            minimal
            icon="diagnosis"
            text="Verify USB Boot…"
            disabled={isDisabled}
            onClick={() => setVerifyBootOpen(true)}
          />
        </div>
      </div>
      <CreateWipeUsbDialog open={wizardOpen} onClose={() => setWizardOpen(false)} />
      <VerifyUsbBootDialog open={verifyBootOpen} onClose={() => setVerifyBootOpen(false)} />
    </section>
  );
}

// ── Row components ────────────────────────────────────────────────

interface StepRowProps {
  def: DestructStepDef;
  checked: boolean;
  onToggle: (id: string, v: boolean) => void;
}

// memo: these rows render 35+ at a time across the columns card: without
// memoizing, every keystroke-unrelated appSettings change (e.g. a sibling
// toggle elsewhere in the app) re-renders all of them. Requires onToggle to
// be a stable reference (writeStep, passed directly — see call site).
const StepRow = memo(function StepRow({ def, checked, onToggle }: StepRowProps) {
  return (
    <label
      className="sd-row"
      onClick={(e) => {
        const target = e.target as HTMLElement;
        // Clicking the checkbox itself: its own onChange (below) handles the
        // toggle — skip the manual call here. (Previously checked
        // `tagName === "INPUT"`, which never matched: the checkbox is a Radix
        // `<button role="checkbox">`, not a native input, so this guard never
        // fired and every click toggled the row twice — once here and once
        // from the checkbox's onChange — doubling the settings-write latency
        // for every single click.)
        if (target.closest('button, [role="checkbox"]')) return;
        // Clicking elsewhere in the row (label text, padding): handle it here,
        // and suppress the native label→control click-forwarding this element
        // would otherwise also perform (it targets the same checkbox), which
        // would otherwise fire onToggle a second time for the same click.
        e.preventDefault();
        onToggle(def.id, !checked);
      }}
    >
      <Checkbox
        className="sd-row-checkbox"
        checked={checked}
        onChange={(e) => onToggle(def.id, (e.target as HTMLInputElement).checked)}
      />
      <div className="sd-row-content">
        <div className={`sd-row-label ${!checked ? "is-disabled" : ""}`}>{def.label}</div>
      </div>
    </label>
  );
});

