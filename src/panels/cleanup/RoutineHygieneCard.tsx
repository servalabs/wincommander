import { Button, Icon } from "@/components/ui/bp";
import { useMemo, useState } from "react";
import { useAppConfirm } from "../../components/shared/AppConfirmDialog";
import {
  DEEP_DFIR_CATEGORIES,
  STANDARD_CATEGORIES,
  type CleanupCategory,
} from "./cleanupCategories";
import {
  buildRoutineHygienePlan,
  getRoutineHygieneCategories,
} from "./hygienePolicy";
import { runCleanupWorkers, type CardData } from "./useCleanupScan";

interface Props {
  cardDataMap: Record<string, CardData>;
  isInvestigator: boolean;
  handleCardLoad: (category: CleanupCategory) => void | Promise<void>;
  handleCardClear: (
    category: CleanupCategory,
    onDriveWipe?: () => void,
  ) => void | Promise<void>;
}

type Operation = "idle" | "scanning" | "clearing";

const ALL_SCAN_CATEGORIES = [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES];

export default function RoutineHygieneCard({
  cardDataMap,
  isInvestigator,
  handleCardLoad,
  handleCardClear,
}: Props) {
  const requestConfirm = useAppConfirm();
  const [operation, setOperation] = useState<Operation>("idle");
  const categories = useMemo(
    () => getRoutineHygieneCategories(ALL_SCAN_CATEGORIES),
    [],
  );
  const plan = useMemo(
    () => buildRoutineHygienePlan(ALL_SCAN_CATEGORIES, cardDataMap),
    [cardDataMap],
  );

  const busy = operation !== "idle" || plan.busy.length > 0;
  const summary = plan.missingCategoryIds.length > 0
    ? "Routine cleanup is unavailable because its reviewed category set is incomplete."
    : plan.failed.length > 0
      ? `${plan.failed.length} cache ${plan.failed.length === 1 ? "category could" : "categories could"} not be previewed. Open the individual card for details.`
      : plan.unscanned.length > 0
        ? `${plan.unscanned.length} cache ${plan.unscanned.length === 1 ? "category needs" : "categories need"} a preview.`
        : plan.busy.length > 0
          ? "Reviewing the selected cache categories…"
          : plan.totalFindings > 0
            ? `${plan.totalFindings} item${plan.totalFindings === 1 ? "" : "s"} across ${plan.ready.length} reviewed cache ${plan.ready.length === 1 ? "category" : "categories"}.`
            : "The reviewed cache categories are clear.";

  const preview = async () => {
    if (busy || isInvestigator || categories.length === 0) return;
    setOperation("scanning");
    try {
      await runCleanupWorkers(
        categories,
        async category => {
          await Promise.resolve(handleCardLoad(category));
        },
        3,
      );
    } finally {
      setOperation("idle");
    }
  };

  const clearReviewedCaches = async () => {
    if (busy || isInvestigator || !plan.canClear) return;
    const labels = plan.ready.map(category => category.label).join(", ");
    const accepted = await requestConfirm({
      title: "Clear reviewed routine caches?",
      description: `Clear ${plan.totalFindings} item${plan.totalFindings === 1 ? "" : "s"} from ${labels}? This preset excludes event and security logs, command and browser history, credentials, profiles, recovery copies, and personal files.`,
      confirmLabel: "Clear reviewed caches",
    });
    if (!accepted) return;

    setOperation("clearing");
    try {
      await runCleanupWorkers(
        plan.ready,
        async category => {
          // The aggregate confirmation above replaces each category's identical
          // cache-only prompt. The normal backend authorization, execution, and
          // post-clear reconciliation path remains unchanged.
          await Promise.resolve(handleCardClear({
            ...category,
            confirmMessage: undefined,
          }));
        },
        1,
      );
    } finally {
      setOperation("idle");
    }
  };

  return (
    <section
      className="mb-8 flex flex-col gap-4"
      aria-labelledby="routine-hygiene-heading"
    >
      <div className="flex items-center gap-3 py-2">
        <h3
          id="routine-hygiene-heading"
          className="whitespace-nowrap text-[10px] font-semibold uppercase tracking-wider"
          style={{ color: "var(--color-text-muted)" }}
        >
          Routine Hygiene
        </h3>
        <div className="h-px flex-1 bg-[var(--color-border)] opacity-50" />
        <span
          className="whitespace-nowrap text-[9px] italic opacity-60"
          style={{ color: "var(--color-text-muted)" }}
        >
          preview first
        </span>
      </div>
      <div className="flex flex-col gap-4">
        <div className="flex flex-col gap-3 rounded-xl border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-4 lg:flex-row lg:items-center lg:justify-between">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <Icon icon="shield" size={16} style={{ color: "var(--color-success)" }} />
              <span className="text-sm font-semibold text-[var(--color-text-primary)]">
                Cache-only bulk cleanup
              </span>
            </div>
            <p className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]" aria-live="polite">
              {summary}
            </p>
          </div>
          <div className="flex shrink-0 flex-wrap gap-2">
            <Button
              icon="search"
              text={plan.previewComplete ? "Preview again" : "Preview routine cleanup"}
              outlined
              loading={operation === "scanning"}
              disabled={busy || isInvestigator || categories.length === 0}
              onClick={() => void preview()}
            />
            <Button
              icon="eraser"
              text="Clear reviewed caches"
              intent="primary"
              loading={operation === "clearing"}
              disabled={busy || isInvestigator || !plan.canClear}
              onClick={() => void clearReviewedCaches()}
            />
          </div>
        </div>

        <div className="flex flex-wrap gap-1.5" aria-label="Routine hygiene categories">
          {categories.map(category => (
            <span
              key={category.id}
              className="rounded-full border border-[var(--color-border)] bg-[var(--color-bg-card)] px-2.5 py-1 text-[10px] font-medium text-[var(--color-text-muted)]"
            >
              {category.label}
            </span>
          ))}
        </div>

        <div className="flex items-start gap-2 text-[11px] leading-5 text-[var(--color-text-muted)]">
          <Icon icon="shield" size={14} className="mt-0.5 shrink-0" />
          <span>
            This guided preset is an explicit allowlist. Every other cleanup category remains an individual expert action with its existing scope, confirmation, entitlement, and review-mode safeguards.
          </span>
        </div>
      </div>
    </section>
  );
}
