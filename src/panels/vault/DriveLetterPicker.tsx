import { useCallback } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/bp";
import type { EngineMountScope } from "./mountScope";
import "./DriveLetterPicker.css";

export type MountTargetMode = "letter" | "folder";

interface DriveLetterPickerProps {
  id: string;
  value: string;
  letters: string[];
  onChange: (letter: string) => void;
  onKeyDown?: React.KeyboardEventHandler<HTMLElement>;
  /** Opt-in mount-target mode. Omitted — every call site before this change
   *  (RAM disks, and the Mount Volume dialog until it adopts folder mounts)
   *  — keeps this exactly the letter-only picker it always was: no mode
   *  toggle renders. Pass this together with `onModeChange` to turn it into
   *  a full Letter/Folder mount-target picker. */
  mode?: MountTargetMode;
  onModeChange?: (mode: MountTargetMode) => void;
  /** Folder mount-point path. Only read/rendered when `mode === "folder"`. */
  mountPoint?: string;
  onMountPointChange?: (path: string) => void;
  /** Mount scope actually in force for this mount. Folder mode needs
   *  SetVolumeMountPoint + the Mount Manager, both of which per-user scope
   *  deliberately avoids (see EncVolMountScope in EncVolFormatSdk.h) — so
   *  "per-user" disables the Folder option and explains why instead of
   *  quietly hiding it. If the caller's `mode` is already "folder" when
   *  `effectiveScope` becomes "per-user", this falls back to the letter grid
   *  (a stale "folder" prop should never strand the user on a dead-end UI);
   *  the caller should still reset its own `mode` state to "letter" when the
   *  scope changes so the two stay in sync. */
  effectiveScope?: EngineMountScope;
}

const FALLBACK_LETTERS = "DEFGHIJKLMNOPQRSTUVWXYZ".split("");
const FOLDER_DISABLED_REASON =
  "Folder mounts need administrator rights and the Mount Manager, which per-user scope deliberately avoids.";

export default function DriveLetterPicker({
  id,
  value,
  letters,
  onChange,
  onKeyDown,
  mode,
  onModeChange,
  mountPoint = "",
  onMountPointChange,
  effectiveScope,
}: DriveLetterPickerProps) {
  const choices = letters.length ? letters : FALLBACK_LETTERS;
  const showModeToggle = mode !== undefined && onModeChange !== undefined;
  const folderDisabled = effectiveScope === "per-user";
  const showFolderInput = mode === "folder" && !folderDisabled;

  const handleBrowseMountPoint = useCallback(async () => {
    try {
      const selected = await open({ directory: true, multiple: false, defaultPath: mountPoint || undefined });
      if (selected && typeof selected === "string") onMountPointChange?.(selected);
    } catch { }
  }, [mountPoint, onMountPointChange]);

  return (
    <div className="mount-target-picker">
      {showModeToggle && (
        <div className="mount-target-toggle" role="tablist" aria-label="Mount target">
          <button
            type="button"
            role="tab"
            aria-selected={mode === "letter"}
            className={`mount-target-toggle-btn${mode === "letter" ? " is-active" : ""}`}
            onClick={() => onModeChange!("letter")}
          >
            Letter
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={mode === "folder"}
            disabled={folderDisabled}
            title={folderDisabled ? FOLDER_DISABLED_REASON : undefined}
            className={`mount-target-toggle-btn${mode === "folder" ? " is-active" : ""}`}
            onClick={() => !folderDisabled && onModeChange!("folder")}
          >
            Folder
          </button>
        </div>
      )}

      {showFolderInput ? (
        <div className="mount-target-folder-row">
          <input
            id={id}
            type="text"
            className="mount-target-folder-input"
            placeholder="C:\Users\You\Vault"
            value={mountPoint}
            autoComplete="off"
            onChange={(e) => onMountPointChange?.(e.target.value)}
            onKeyDown={onKeyDown}
          />
          <Button icon="folder-open" minimal aria-label="Browse for a mount-point folder" onClick={handleBrowseMountPoint} />
        </div>
      ) : (
        <div id={id} className="drive-letter-picker" role="radiogroup" aria-label="Drive letter">
          {choices.map((letter) => (
            <button
              key={letter}
              type="button"
              role="radio"
              aria-checked={value === letter}
              className={`drive-letter-picker__option ${value === letter ? "drive-letter-picker__option--selected" : ""}`}
              onClick={() => onChange(letter)}
              onKeyDown={onKeyDown}
            >
              {letter}:
            </button>
          ))}
        </div>
      )}

      {showModeToggle && folderDisabled && mode === "folder" && (
        <p className="mount-target-hint">{FOLDER_DISABLED_REASON}</p>
      )}
    </div>
  );
}
