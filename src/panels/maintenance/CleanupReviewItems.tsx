import { useState } from "react";
import { Button } from "../../components/ui/button";

export function CleanupReviewItems({ items }: { items: { id: string; path: string; detail: string }[] }) {
  const [visibleCount, setVisibleCount] = useState(50);
  return <div className="max-h-64 overflow-y-auto rounded-[var(--r)] border border-[var(--border)] p-3 text-xs">
    <ul aria-label="Items selected for removal">{items.slice(0, visibleCount).map((item) => <li key={item.id} className="border-b border-[var(--border)] py-2 last:border-0"><span className="block break-all font-mono">{item.path}</span><span className="text-[var(--text-dim)]">{item.detail}</span></li>)}</ul>
    {items.length > visibleCount && <Button size="sm" variant="outline" onClick={() => setVisibleCount((count) => count + 50)}>Review more ({items.length - visibleCount} remaining)</Button>}
  </div>;
}
