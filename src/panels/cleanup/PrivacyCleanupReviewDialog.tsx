import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from "../../components/ui/alert-dialog";
import { CleanupReviewItems } from "../maintenance/CleanupReviewItems";

export interface PrivacyClearReview {
  category: string;
  scope: string;
  count: number;
  items: string[];
  run: () => void;
}

export function PrivacyCleanupReviewDialog({ review, onClose }: { review: PrivacyClearReview | null; onClose: () => void }) {
  return <AlertDialog open={!!review} onOpenChange={(open) => { if (!open) onClose(); }}>
    <AlertDialogContent>
      <AlertDialogHeader><AlertDialogTitle>Review {review?.category}</AlertDialogTitle><AlertDialogDescription>Scope: {review?.scope}. This action clears the entire supported category, including records created since this preview. Individual record selection is unavailable. Review the records and category confirmation before continuing.</AlertDialogDescription></AlertDialogHeader>
      {review && <>
        <p className="text-xs text-[var(--text-dim)]">{review.count.toLocaleString()} reported records · {review.items.length.toLocaleString()} available preview entries. {review.items.length < review.count && "The backend does not return every record; this preview is incomplete."}</p>
        <CleanupReviewItems items={review.items.map((item, index) => ({ id: String(index), path: item, detail: "Observed record" }))} />
      </>}
      <AlertDialogFooter><AlertDialogCancel>Back</AlertDialogCancel><AlertDialogAction onClick={() => { const run = review?.run; onClose(); run?.(); }}>Clear reviewed category</AlertDialogAction></AlertDialogFooter>
    </AlertDialogContent>
  </AlertDialog>;
}
