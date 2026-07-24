import { Button, Icon } from "@/components/ui/bp";
import type { CleanupCategory } from "../../panels/cleanup/cleanupCategories";
import "./TraceDetailDialog.css";

interface TraceDetailDialogProps {
  category: Pick<CleanupCategory,
    "label" | "description" | "icon" | "severity" | "scopeAware" | "systemWide" |
    "schedulable" | "minIntervalMinutes" | "schedulerRunAsSystem" | "regeneratesNote" |
    "clearDataKey"
  >;
  isOpen: boolean;
  count: number;
  items: string[];
  groupedItems?: Array<{ title: string; count: number; items: string[] }>;
  clearing: boolean;
  onClose: () => void;
  onClear?: () => void;
  clearDisabled?: boolean;
  clearLabel?: string;
}

export default function TraceDetailDialog({
  category,
  isOpen,
  count,
  items,
  groupedItems,
  clearing,
  onClose,
  onClear,
  clearDisabled,
  clearLabel = "Clear",
}: TraceDetailDialogProps) {
  if (!isOpen) {
    return null;
  }

  return (
    <div className="trace-dialog" role="dialog" aria-modal="true" aria-label={category.label}>
      <div className="trace-dialog__panel">
        <header className="trace-dialog__header">
          <div className={`trace-dialog__icon trace-dialog__icon--${category.severity}`}>
            <Icon icon={category.icon as never} size={18} />
          </div>
          <div>
            <h2>{category.label}</h2>
            <p>{category.description}</p>
          </div>
          <div className="trace-dialog__actions">
            <Button icon="cross" minimal aria-label="Close" onClick={onClose} />
          </div>
        </header>
        <div className="trace-dialog__summary">
          <strong>{count}</strong>
          <span>{count === 1 ? "item" : "items"}</span>
        </div>
        <div className="trace-dialog__items">
          {groupedItems?.length ? (
            Array.from(groupedItems.entries()).map(([gi, group]) => (
              <section className="trace-dialog__group" key={`${group.title}-${gi}`}>
                <div className="trace-dialog__group-header">
                  <strong>{group.title}</strong>
                  <span>{group.count} {group.count === 1 ? "item" : "items"}</span>
                </div>
                <div className="trace-dialog__group-items">
                  {group.items.length ? (
                    Array.from(group.items.entries()).map(([ii, item]) => <code key={`${group.title}:${item}-${ii}`}>{item}</code>)
                  ) : (
                    <span className="trace-dialog__empty">No entries found for this user.</span>
                  )}
                </div>
              </section>
            ))
          ) : items.length ? (
            Array.from(items.entries()).map(([ii, item]) => <code key={`${item}-${ii}`}>{item}</code>)
          ) : (
            <span className="trace-dialog__empty">No entries found.</span>
          )}
        </div>
        <footer className="trace-dialog__footer">
          <Button text="Close" minimal onClick={onClose} />
          {onClear ? (
            <Button
              text={clearLabel}
              intent="danger"
              loading={clearing}
              disabled={clearDisabled || clearing}
              onClick={onClear}
            />
          ) : null}
        </footer>
      </div>
    </div>
  );
}
