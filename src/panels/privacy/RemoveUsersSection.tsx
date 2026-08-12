// src/panels/privacy/RemoveUsersSection.tsx
//
// Selection UI for the "Remove Users & Wipe Data" lockdown step
// (registry id "remove_users", group privacyClean). Lets an admin
// pre-select local Windows accounts that get securely wiped +
// deleted when the self-destruct cascade fires.
//
// Pure prop-driven renderer — no appState reads, no SectionCard/
// TierGate of its own (the parent LockdownConfigSection already
// wraps everything in <TierGate tier="paid">). Mirrors the existing
// "Folders to Delete on Lockdown" block for visual rhythm and reuses
// the sd-row checkbox-row family from LockdownConfigSection.
//
// Settings patch semantics: the patch endpoint deep-merges objects
// but REPLACES arrays wholesale and can't delete keys, so every
// selection change must send the FULL desired usersToRemove array
// (see removeUsersUtils.toggleUser) — never a delta.

import { useCallback, useEffect, useState } from "react";
import { Button, Checkbox, Icon, Spinner } from "@/components/ui/bp";
import { executeBackendCommand } from "../../hooks/useBackend";
import { showError } from "../../utils/toast";
import {
  filterAndOrderLocalUsers,
  type LocalLoginUser,
} from "../../components/tweaks/managers/localUsersManagerUtils";
import { isUserSelectable, toggleUser } from "./removeUsersUtils";
import "./LockdownConfigSection.css";

interface Props {
  usersToRemove: string[];
  onPatch: (patch: { usersToRemove: string[] }) => void;
}

function normalizeLocalUsers(data: unknown): LocalLoginUser[] {
  if (!data) return [];
  if (Array.isArray(data)) return data as LocalLoginUser[];
  if (typeof data === "object") return [data as LocalLoginUser];
  return [];
}

function selectionReason(user: LocalLoginUser): string | null {
  if (user.currentUser) return "current account";
  if (user.builtIn) return "built-in";
  return null;
}

export default function RemoveUsersSection({ usersToRemove, onPatch }: Props) {
  const [rows, setRows] = useState<LocalLoginUser[]>([]);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    const res = await executeBackendCommand<LocalLoginUser[] | LocalLoginUser>("Get-LocalLoginUsers");
    setLoading(false);
    if (res.success) {
      setRows(normalizeLocalUsers(res.data));
    } else {
      showError(res.error || "Failed to load local users");
    }
  }, []);

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleToggle = useCallback(
    (user: LocalLoginUser) => {
      if (!isUserSelectable(user)) return;
      onPatch({ usersToRemove: toggleUser(usersToRemove, user.name) });
    },
    [usersToRemove, onPatch],
  );

  const ordered = filterAndOrderLocalUsers(rows, "");

  return (
    <div className="sd-shred-folders">
      <div className="sd-shred-folders-header">
        <Icon icon="people" size={14} className="sd-shred-folders-icon" />
        <div className="sd-shred-folders-text">
          <div className="sd-shred-folders-label">Users to Remove on Lockdown</div>
        </div>
        <Button
          className="sd-bulk-btn"
          minimal
          small
          icon="refresh"
          text="Reload"
          loading={loading}
          onClick={refresh}
        />
      </div>

      <p className="sd-remove-users-warning">
        Selected accounts and all their data are permanently deleted when lockdown fires.
        Built-in and signed-in accounts are always skipped.
      </p>

      {loading && rows.length === 0 && <Spinner size={20} />}

      {!loading && ordered.length === 0 && (
        <div className="sd-remove-users-empty">No local users found.</div>
      )}

      {ordered.length > 0 && (
        <div className="sd-remove-users-list">
          {ordered.map((user) => {
            const selectable = isUserSelectable(user);
            const reason = selectionReason(user);
            const checked = usersToRemove.includes(user.name);
            return (
              <label
                key={user.sid || user.name}
                className="sd-row"
                onClick={(e) => {
                  const target = e.target as HTMLElement;
                  if (target.closest('button, [role="checkbox"]')) return;
                  e.preventDefault();
                  handleToggle(user);
                }}
              >
                <Checkbox
                  className="sd-row-checkbox"
                  checked={checked}
                  disabled={!selectable}
                  onChange={() => handleToggle(user)}
                />
                <div className="sd-row-content">
                  <div className={`sd-row-label ${!selectable ? "is-disabled" : ""}`}>
                    {user.name}
                    {user.fullName && (
                      <span className="sd-remove-users-fullname">{user.fullName}</span>
                    )}
                  </div>
                  {reason && <div className="sd-row-desc">{reason}</div>}
                </div>
              </label>
            );
          })}
        </div>
      )}
    </div>
  );
}
