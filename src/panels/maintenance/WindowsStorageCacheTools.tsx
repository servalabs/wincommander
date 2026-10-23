import { useState } from "react";
import CleanupTraceCard from "../../components/cleanup/CleanupTraceCard";
import TraceDetailDialog from "../../components/shared/TraceDetailDialog";
import { DEEP_DFIR_CATEGORIES, STANDARD_CATEGORIES, type CleanupCategory } from "../cleanup/cleanupCategories";
import { PrivacyCleanupReviewDialog, type PrivacyClearReview } from "../cleanup/PrivacyCleanupReviewDialog";
import type { CardData } from "../cleanup/useCleanupScan";
import { getWindowsStorageCacheCategories } from "./windowsStorageCachePolicy";

interface Props {
  cardDataMap: Record<string, CardData>;
  isInvestigator: boolean;
  handleCardLoad: (category: CleanupCategory) => void | Promise<void>;
  handleCardClear: (category: CleanupCategory) => void | Promise<void>;
}

const CATEGORIES = getWindowsStorageCacheCategories([...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES]);

export default function WindowsStorageCacheTools({ cardDataMap, isInvestigator, handleCardLoad, handleCardClear }: Props) {
  const [review, setReview] = useState<PrivacyClearReview | null>(null);
  const [detail, setDetail] = useState<CleanupCategory | null>(null);
  const detailData = detail ? cardDataMap[detail.id] : undefined;

  return <section aria-label="Additional Windows caches" className="flex flex-col gap-3">
    <p className="text-xs leading-5 text-[var(--text-mute)]">Review Spotlight images, the legacy Explorer icon database and BranchCache separately. These are outside the selected storage categories above; Windows Clean all may also include BranchCache. DNS, location records and app state remain individual actions in System Cleanup.</p>
    <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
      {CATEGORIES.map((category) => {
        const data = cardDataMap[category.id];
        const count = data?.count ?? -1;
        const canClear = !isInvestigator && count > 0 && !data?.loading && !data?.clearing && !data?.error;
        const openReview = () => { if (!canClear) return; setReview({
          category: category.label,
          scope: category.systemWide ? "Machine scope" : "Current Windows account",
          count,
          items: data?.items ?? [],
          run: () => void handleCardClear(category),
        }); };
        return <CleanupTraceCard
          key={category.id}
          category={category}
          count={count}
          preview={(data?.items ?? []).slice(0, 3)}
          loading={data?.loading ?? false}
          clearing={data?.clearing ?? false}
          error={data?.error}
          clearDisabled={!canClear}
          onLoad={() => void handleCardLoad(category)}
          onViewDetails={() => setDetail(category)}
          onClear={openReview}
        />;
      })}
    </div>
    {detail && <TraceDetailDialog category={detail} isOpen count={detailData?.count ?? 0} items={detailData?.items ?? []} rawData={detailData?.raw} clearing={detailData?.clearing ?? false} onClose={() => setDetail(null)} />}
    <PrivacyCleanupReviewDialog review={review} onClose={() => setReview(null)} />
  </section>;
}
