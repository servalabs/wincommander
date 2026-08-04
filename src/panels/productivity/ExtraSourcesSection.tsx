// src/panels/productivity/ExtraSourcesSection.tsx
//
// Native views for ActivityWatch bucket types beyond window/AFK — browser
// (aw-watcher-web: full URLs + page titles), VS Code (aw-watcher-vscode:
// project/file/language), input-count aggregates (aw-watcher-input: no
// keystroke content, ActivityWatch doesn't capture it), and a generic
// fallback for any other bucket type. Rendered beneath the copied
// WinCommanderActivityProductivity component on the Activity tab.
// Aggregation lives in src/lib/activityWatchExtras.ts — this file only
// renders, and only as text nodes (no links, no dangerouslySetInnerHTML).

import { H5, Text } from "@/components/ui/bp";
import { ActivityRankedList } from "@/components/activity/ActivityRankedList";
import type { GenericBucketSummary, InputSummary, VscodeSummary, WebSummary } from "@/lib/activityWatchExtras";

interface ExtraSourcesSectionProps {
  web: WebSummary | null;
  vscode: VscodeSummary | null;
  input: InputSummary | null;
  generic: GenericBucketSummary[];
  emptyMessage: string;
}

export default function ExtraSourcesSection({ web, vscode, input, generic, emptyMessage }: ExtraSourcesSectionProps) {
  if (!web && !vscode && !input && generic.length === 0) return null;

  return (
    <div className="productivity-extra-sources">
      {web && (
        <div className="productivity-extra-grid">
          <ActivityRankedList title="Top Websites" items={web.topUrls} emptyMessage={emptyMessage} />
          <ActivityRankedList title="Top Page Titles" items={web.topTitles} emptyMessage={emptyMessage} />
        </div>
      )}
      {vscode && (
        <div className="productivity-extra-grid">
          <ActivityRankedList title="Top Projects" items={vscode.topProjects} emptyMessage={emptyMessage} />
          <ActivityRankedList title="Top Files" items={vscode.topFiles} emptyMessage={emptyMessage} />
          <ActivityRankedList title="Top Languages" items={vscode.topLanguages} emptyMessage={emptyMessage} />
        </div>
      )}
      {input && (
        <div className="productivity-input-stats" aria-label="Input activity">
          <H5>Input Activity</H5>
          <div className="productivity-input-stats-row">
            <span><strong>{input.presses.toLocaleString()}</strong> key presses</span>
            <span><strong>{input.clicks.toLocaleString()}</strong> clicks</span>
            {input.scrolls > 0 && <span><strong>{input.scrolls.toLocaleString()}</strong> scrolls</span>}
            <span><strong>{input.movementPx.toLocaleString()}</strong> px moved</span>
          </div>
          <Text className="productivity-input-stats-note">Aggregate counts only — ActivityWatch does not record what was typed.</Text>
        </div>
      )}
      {generic.length > 0 && (
        <div className="productivity-generic-sources" aria-label="Other data sources">
          <H5>Other Sources</H5>
          {generic.map((bucket) => (
            <div key={bucket.bucketId} className="productivity-generic-bucket">
              <Text className="productivity-generic-bucket-title">
                {bucket.bucketId} ({bucket.bucketType}) — {bucket.eventCount} events
              </Text>
              {bucket.samples.map((sample, index) => (
                <Text key={index} className="productivity-generic-sample">{sample}</Text>
              ))}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
