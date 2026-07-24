import { useEffect } from "react";
import { Icon } from "@/components/ui/icon";
import type { SearchContextAction, SearchContextTarget } from "@/hooks/useSearchResultContextMenu";

interface Props {
  target: SearchContextTarget;
  onAction: (action: SearchContextAction) => void;
  onClose: () => void;
}

const ACTIONS: Array<{ id: SearchContextAction; label: string; icon: string; fileOnly?: boolean }> = [
  { id: "open", label: "Open", icon: "document" },
  { id: "open-folder", label: "Open containing folder", icon: "folder-open", fileOnly: true },
  { id: "copy", label: "Copy", icon: "copy", fileOnly: true },
  { id: "cut", label: "Cut", icon: "cut", fileOnly: true },
  { id: "copy-path", label: "Copy path", icon: "clipboard", fileOnly: true },
  { id: "vscode", label: "Open in Visual Studio Code", icon: "code", fileOnly: true },
];

export default function SearchResultContextMenu({ target, onAction, onClose }: Props) {
  useEffect(() => {
    const close = () => onClose();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
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
  }, [onClose]);

  return (
    <div
      className="esb-context-menu"
      style={{ left: target.x, top: target.y }}
      role="menu"
      aria-label={`Actions for ${target.label}`}
      onPointerDown={(event) => event.stopPropagation()}
      onContextMenu={(event) => event.preventDefault()}
    >
      <div className="esb-context-label" title={target.path}>{target.label}</div>
      {ACTIONS.map((action) => (
        <button
          key={action.id}
          type="button"
          role="menuitem"
          className="esb-context-action"
          disabled={action.fileOnly && !target.canUseFileActions}
          onClick={() => onAction(action.id)}
        >
          <Icon icon={action.icon} size={15} />
          <span>{action.label}</span>
        </button>
      ))}
    </div>
  );
}
