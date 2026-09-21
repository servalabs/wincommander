import { useMemo } from "react";
import { EyeOff, RotateCcw } from "lucide-react";
import { CompatDialog, CompatDialogBody, CompatDialogFooter } from "../ui/compat-dialog";
import type { ScanFinding } from "../startup/WizardAnimations";

export interface IgnoredFinding {
  id: string;
  label: string;
  impact?: string;
}

export interface IgnoredFindingsDialogProps {
  isOpen: boolean;
  ignoredFindingIds: readonly string[];
  knownFindings: readonly ScanFinding[];
  onClose: () => void;
  onRestore: (id: string) => void;
}

/**
 * Settings persist finding IDs, not copies of the finding. Resolve a current
 * label where the scan still knows it; retain the ID as a truthful fallback
 * for a finding that is not presently detectable.
 */
export function resolveIgnoredFindings(
  ignoredFindingIds: readonly string[],
  knownFindings: readonly ScanFinding[],
): IgnoredFinding[] {
  const byId = new Map(knownFindings.map((finding) => [finding.id, finding]));
  return [...new Set(ignoredFindingIds)].map((id) => {
    const finding = byId.get(id);
    return {
      id,
      label: finding?.label ?? id,
      impact: finding?.impact,
    };
  });
}

export default function IgnoredFindingsDialog({
  isOpen,
  ignoredFindingIds,
  knownFindings,
  onClose,
  onRestore,
}: IgnoredFindingsDialogProps) {
  const ignoredFindings = useMemo(
    () => resolveIgnoredFindings(ignoredFindingIds, knownFindings),
    [ignoredFindingIds, knownFindings],
  );

  return (
    <CompatDialog
      isOpen={isOpen}
      onClose={onClose}
      title="Ignored Fix All items"
      icon="eye-off"
      className="ignored-findings-dialog"
    >
      <CompatDialogBody className="ignored-findings-dialog__body">
        <p className="ignored-findings-dialog__intro">
          Ignored items stay out of Fix All until you restore them. Restoring an item does not change Windows; it only makes the recommendation visible again.
        </p>
        {ignoredFindings.length === 0 ? (
          <p className="ignored-findings-dialog__empty">No items are ignored.</p>
        ) : (
          <ul className="ignored-findings-dialog__list" aria-label="Ignored Fix All items">
            {ignoredFindings.map((finding) => (
              <li className="ignored-findings-dialog__item" key={finding.id}>
                <EyeOff size={15} aria-hidden="true" />
                <div className="ignored-findings-dialog__details">
                  <strong>{finding.label}</strong>
                  {finding.impact ? <span>{finding.impact}</span> : null}
                  {finding.label === finding.id ? <code>{finding.id}</code> : null}
                </div>
                <button type="button" className="ignored-findings-dialog__restore" onClick={() => onRestore(finding.id)}>
                  <RotateCcw size={13} aria-hidden="true" />
                  Restore
                </button>
              </li>
            ))}
          </ul>
        )}
      </CompatDialogBody>
      <CompatDialogFooter>
        <button type="button" className="ignored-findings-dialog__close" onClick={onClose}>Close</button>
      </CompatDialogFooter>
    </CompatDialog>
  );
}
