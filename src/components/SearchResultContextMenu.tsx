import { Fragment, useEffect, useRef, useState } from "react";
import { Icon } from "@/components/ui/icon";
import { Separator } from "@/components/ui/separator";
import type { SearchContextAction, SearchContextTarget } from "@/hooks/useSearchResultContextMenu";

interface Props {
  target: SearchContextTarget;
  onAction: (action: SearchContextAction, payload?: string) => void;
  onClose: () => void;
}

const ACTIONS: Array<{ id: SearchContextAction; label: string; icon: string; fileOnly?: boolean; separatorBefore?: boolean }> = [
  { id: "open", label: "Open", icon: "document" },
  { id: "open-folder", label: "Open containing folder", icon: "folder-open", fileOnly: true },
  { id: "copy", label: "Copy", icon: "duplicate", fileOnly: true },
  { id: "cut", label: "Cut", icon: "cut", fileOnly: true },
  { id: "copy-path", label: "Copy path", icon: "clipboard", fileOnly: true },
  { id: "vscode", label: "Open in Visual Studio Code", icon: "code", fileOnly: true },
  { id: "rename", label: "Rename", icon: "edit", fileOnly: true },
  { id: "properties", label: "Properties", icon: "info-sign", fileOnly: true },
  // Destructive actions last, separated from the actions above.
  { id: "delete", label: "Delete", icon: "trash", fileOnly: true, separatorBefore: true },
  { id: "shred", label: "Shred (permanent)", icon: "flame", fileOnly: true },
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
      className="esb-context-menu custom-scrollbar"
      style={{ left: target.x, top: target.y }}
      role="menu"
      aria-label={`Actions for ${target.label}`}
      onPointerDown={(event) => event.stopPropagation()}
      onContextMenu={(event) => event.preventDefault()}
    >
      <div className="esb-context-label" title={target.path}>{target.label}</div>
      {renaming ? (
        <form
          className="flex flex-col gap-2 px-2 pb-2 pt-1"
          onSubmit={(event) => { event.preventDefault(); confirmRename(); }}
        >
          <input
            ref={renameInputRef}
            autoFocus
            className="w-full rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--surface)] px-2 py-1.5 text-[12px] text-[var(--text)] outline-none focus:border-[var(--accent)]"
            value={renameValue}
            onChange={(event) => setRenameValue(event.target.value)}
          />
          <div className="flex justify-end gap-1.5">
            <button
              type="button"
              className="rounded-[var(--radius-md)] px-2.5 py-1 text-[12px] text-[var(--text-mute)] hover:bg-[var(--accent-soft)] hover:text-[var(--accent)]"
              onClick={() => setRenaming(false)}
            >
              Cancel
            </button>
            <button
              type="submit"
              className="rounded-[var(--radius-md)] bg-[var(--accent-soft)] px-2.5 py-1 text-[12px] font-medium text-[var(--accent)] hover:bg-[var(--accent)] hover:text-[var(--surface)]"
            >
              Confirm
            </button>
          </div>
        </form>
      ) : (
        ACTIONS.map((action) => (
          <Fragment key={action.id}>
            {action.separatorBefore && <Separator className="my-1" />}
            <button
              type="button"
              role="menuitem"
              className="esb-context-action"
              disabled={action.fileOnly && !target.canUseFileActions}
              onClick={() => (action.id === "rename" ? startRename() : onAction(action.id))}
            >
              <Icon icon={action.icon} size={15} />
              <span>{action.label}</span>
            </button>
          </Fragment>
        ))
      )}
    </div>
  );
}
