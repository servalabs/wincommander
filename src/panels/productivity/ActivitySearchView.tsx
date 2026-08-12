// src/panels/productivity/ActivitySearchView.tsx
//
// "Search" tab: a plain substring filter over the day's combined activity
// log (app/title/detail/category) — replaces ActivityWatch's own
// query-language /#/search page. No DSL: the owner wants this readable by
// anyone, not just people who know AQL.

import { useMemo, useState } from "react";
import { InputGroup } from "@/components/ui/bp";
import ActivityTimelineLog from "./ActivityTimelineLog";
import type { CombinedActivityEvent } from "@/hooks/useActivityWatchDay";

interface ActivitySearchViewProps {
  events: CombinedActivityEvent[];
}

function matchesQuery(event: CombinedActivityEvent, query: string): boolean {
  const haystack = `${event.title} ${event.detail} ${event.categoryPath?.join(" ") ?? ""}`.toLowerCase();
  return haystack.includes(query);
}

export default function ActivitySearchView({ events }: ActivitySearchViewProps) {
  const [query, setQuery] = useState("");
  const trimmed = query.trim().toLowerCase();
  const filtered = useMemo(
    () => (trimmed ? events.filter((event) => matchesQuery(event, trimmed)) : events),
    [events, trimmed],
  );

  return (
    <div className="productivity-search-view">
      <InputGroup
        leftIcon="search"
        placeholder="Search app names, window titles, URLs, files…"
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        aria-label="Search activity"
        fill
      />
      <ActivityTimelineLog
        events={filtered}
        emptyMessage={trimmed ? `No activity matched "${query.trim()}".` : "No activity recorded for this date."}
      />
    </div>
  );
}
