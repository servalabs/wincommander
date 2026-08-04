// src/components/activity/ActivityRankedList.tsx
//
// Ranked "Top X" list for the local Productivity panel. AGPL-3.0, part of the
// Free app's own viewer layer.
//
// The Pro fleet console has a separate implementation of a similar list under
// the WinCommander EULA. Do NOT sync or diff the two: `OPEN_CORE.md` places
// fleet services on the proprietary side of the boundary and this file on the
// public side, so keeping them independent is intentional.

import { useEffect, useMemo, useState } from "react";
import type { ActivityItem } from "./activityData";
import { formatActivityDuration } from "./activityData";

type Props = {
  title: string;
  items: ActivityItem[];
  emptyMessage: string;
  initialItems?: number;
};

export function ActivityRankedList({ title, items, emptyMessage, initialItems = 5 }: Props) {
  const ranked = useMemo(
    () => [...items].filter((item) => item.seconds > 0).sort((left, right) => right.seconds - left.seconds),
    [items],
  );
  const [limit, setLimit] = useState(initialItems);

  useEffect(() => setLimit(initialItems), [initialItems, items]);

  const visible = ranked.slice(0, limit);
  const longest = Math.max(1, ...visible.map((item) => item.seconds));
  const canShowMore = limit < ranked.length;
  const canCollapse = limit > initialItems;

  return (
    <section className="wc-fleet-section" aria-label={title}>
      <h3>{title}</h3>
      {visible.length === 0 ? <p className="wc-fleet-empty">{emptyMessage}</p> : (
        <ol className="wc-fleet-ranked-list">
          {visible.map((item, index) => (
            <li
              key={item.id ?? `${item.label}-${index}`}
              className="wc-fleet-ranked-item"
              title={`${item.label}\n${formatActivityDuration(item.seconds)}`}
            >
              <span
                className="wc-fleet-ranked-fill"
                style={{ width: `${Math.max(2, item.seconds / longest * 100)}%` }}
                aria-hidden="true"
              />
              <span className="wc-fleet-ranked-label">{item.label}</span>
              <time>{formatActivityDuration(item.seconds)}</time>
            </li>
          ))}
        </ol>
      )}
      {(canShowMore || canCollapse) && (
        <div className="wc-fleet-list-actions">
          {canShowMore && <button type="button" onClick={() => setLimit((value) => value + 5)}>⌄ Show more</button>}
          {canCollapse && <button type="button" aria-label={`Show fewer ${title}`} onClick={() => setLimit(initialItems)}>⌃</button>}
        </div>
      )}
    </section>
  );
}
