import { useEffect, useRef, useState } from "react";
import { Icon } from "@/components/ui/icon";
import type { SearchContextAction, SearchContextTarget } from "@/hooks/useSearchResultContextMenu";

interface Props {
  target: SearchContextTarget;
  onAction: (action: SearchContextAction, payload?: string) => void;
  onClose: () => void;
}

const PRIMARY_ACTIONS: Array<{ id: SearchContextAction; label: string; icon: string }> = [
  { id: "open", label: "Open", icon: "document" },
  { id: "open-folder", label: "Folder", icon: "folder-open" },
  { id: "vscode", label: "Code", icon: "code" },
];

const COMPACT_ROWS: Array<Array<{ id: SearchContextAction; label: string; icon: string; danger?: boolean }>> = [
  [{ id: "copy", label: "Copy", icon: "duplicate" }, { id: "copy-path", label: "Copy path", icon: "clipboard" }],
  [{ id: "cut", label: "Cut", icon: "cut" }, { id: "rename", label: "Rename", icon: "edit" }],
  [{ id: "delete", label: "Delete", icon: "trash", danger: true }, { id: "shred", label: "Shred", icon: "flame", danger: true }],
];

export default function SearchResultContextMenu({ target, onAction, onClose }: Props) {
  const [renaming, setRenaming] = useState(false);
  const [renameValue, setRenameValue] = useState(target.label);
  const renameInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const close = () => onClose();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      // Escape backs out of the rename prompt first, then closes the menu.
      if (renaming) { setRenaming(false); return; }
      close();
    };
    window.addEventListener("pointerdown", close);
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      window.removeEventListener("pointerdown", close);
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
      window.removeEventListener("keydown", onKeyDown, true);
    };
  }, [onClose, renaming]);

  useEffect(() => {
    if (!renaming) return;

    // Focus after the rename form commits, then once more after paint. The
    // search overlay uses a short retry loop to recover native WebView focus
    // after a global hotkey; its context-menu guard stops that loop, while
    // these two attempts make the inline editor reliable in both windows.
    const focusRenameInput = () => {
      renameInputRef.current?.focus();
      renameInputRef.current?.select();
    };
    focusRenameInput();
    const frame = requestAnimationFrame(focusRenameInput);
    return () => cancelAnimationFrame(frame);
  }, [renaming]);

  const startRename = () => {
    setRenameValue(target.label);
    setRenaming(true);
  };

  const confirmRename = () => {
    const trimmed = renameValue.trim();
    if (!trimmed || trimmed === target.label) {
      setRenaming(false);
      return;
    }
    onAction("rename", trimmed);
  };

  return (
    <div
      className="esb-shortcut-context-menu custom-scrollbar"
      style={{ left: target.x, top: target.y }}
      role="menu"
      aria-label={`Actions for ${target.label}`}
      onPointerDown={(event) => event.stopPropagation()}
      onContextMenu={(event) => event.preventDefault()}
    >
      <div className="esb-shortcut-context-toolbar">
        <div className="esb-shortcut-context-label" title={target.path}>{target.label}</div>
        <button
          type="button"
          className="esb-shortcut-context-properties"
          disabled={!target.canUseFileActions}
          aria-label="Properties"
          title="Properties"
          onClick={() => onAction("properties")}
        >
          <Icon icon="info-sign" size={14} />
        </button>
      </div>
      {renaming ? (
        <form
          className="esb-shortcut-rename"
          onSubmit={(event) => { event.preventDefault(); confirmRename(); }}
        >
          <input
            ref={renameInputRef}
            autoFocus
            className="esb-shortcut-rename-input"
            value={renameValue}
            onChange={(event) => setRenameValue(event.target.value)}
          />
          <div className="esb-shortcut-rename-actions">
            <button
              type="button"
              className="esb-shortcut-rename-cancel"
              onClick={() => setRenaming(false)}
            >
              Cancel
            </button>
            <button
              type="submit"
              className="esb-shortcut-rename-confirm"
            >
              Confirm
            </button>
          </div>
        </form>
      ) : (
        <div className="esb-shortcut-context-groups">
          <div className="esb-shortcut-context-primary">
            {PRIMARY_ACTIONS.map((action) => (
              <button
                key={action.id}
                type="button"
                role="menuitem"
                className="esb-shortcut-context-action"
                disabled={action.id !== "open" && !target.canUseFileActions}
                onClick={() => onAction(action.id)}
              >
                <Icon icon={action.icon} size={14} />
                <span>{action.label}</span>
              </button>
            ))}
          </div>
          {COMPACT_ROWS.map((row) => (
            <div key={row[0].id} className="esb-shortcut-context-row">
              {row.map((action) => (
                <button
                  key={action.id}
                  type="button"
                  role="menuitem"
                  className="esb-shortcut-context-action"
                  data-danger={action.danger || undefined}
                  disabled={!target.canUseFileActions}
                  onClick={() => (action.id === "rename" ? startRename() : onAction(action.id))}
                >
                  <Icon icon={action.icon} size={14} />
                  <span>{action.label}</span>
                </button>
              ))}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
