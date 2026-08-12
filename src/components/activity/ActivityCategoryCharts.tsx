// src/components/activity/ActivityCategoryCharts.tsx
//
// Category tree + sunburst for the local Productivity panel. AGPL-3.0, part of
// the Free app's own viewer layer.
//
// The Pro fleet console has a separate implementation under the WinCommander
// EULA. Do NOT sync or diff the two: `OPEN_CORE.md` places fleet services on
// the proprietary side of the boundary and this file on the public side.

import { useMemo, useState } from "react";
import { ActivityRankedList } from "./ActivityRankedList";
import {
  flattenCategoryActivity,
  formatActivityDuration,
  sumActivity,
  type ActivityCategory,
} from "./activityData";

type Props = { categories: ActivityCategory[]; emptyMessage: string };
type Slice = {
  node: ActivityCategory;
  start: number;
  end: number;
  inner: number;
  outer: number;
  color: string;
  path: string[];
};

const FALLBACK_COLOURS = ["var(--text-mute)", "var(--accent)", "var(--ok)", "var(--warn)", "var(--danger)"];

function categoryColour(category: ActivityCategory, index: number): string {
  return category.color || FALLBACK_COLOURS[index % FALLBACK_COLOURS.length];
}

function CategoryTree({ categories, total, showPercent }: {
  categories: ActivityCategory[];
  total: number;
  showPercent: boolean;
}) {
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());

  function renderNodes(nodes: ActivityCategory[]) {
    return nodes.map((category, index) => {
      const isCollapsed = collapsed.has(category.id);
      const hasChildren = category.children.length > 0;
      return (
        <li key={category.id}>
          <div className="wc-fleet-category-row">
            {hasChildren ? (
              <button
                type="button"
                aria-label={`${isCollapsed ? "Expand" : "Collapse"} ${category.label}`}
                aria-expanded={!isCollapsed}
                onClick={() => setCollapsed((current) => {
                  const next = new Set(current);
                  if (next.has(category.id)) next.delete(category.id); else next.add(category.id);
                  return next;
                })}
              >{isCollapsed ? "+" : "−"}</button>
            ) : <i style={{ background: categoryColour(category, index) }} />}
            <span>{category.label}</span>
            <time>{showPercent && total > 0
              ? `${Math.round(category.seconds / total * 100)}%`
              : formatActivityDuration(category.seconds)}</time>
          </div>
          {hasChildren && !isCollapsed && <ul>{renderNodes(category.children)}</ul>}
        </li>
      );
    });
  }

  return <ul className="wc-fleet-category-tree">{renderNodes(categories)}</ul>;
}

function findCategory(categories: ActivityCategory[], id: string): ActivityCategory | null {
  for (const category of categories) {
    if (category.id === id) return category;
    const child = findCategory(category.children, id);
    if (child) return child;
  }
  return null;
}

function maxDepth(categories: ActivityCategory[]): number {
  return categories.reduce((depth, category) => Math.max(depth, 1 + maxDepth(category.children)), 0);
}

function createSlices(categories: ActivityCategory[]): Slice[] {
  const slices: Slice[] = [];
  const depth = Math.max(1, maxDepth(categories));
  const ringWidth = 54 / depth;

  function walk(nodes: ActivityCategory[], rangeStart: number, rangeEnd: number, level: number, path: string[]) {
    const total = sumActivity(nodes);
    if (!total) return;
    let cursor = rangeStart;
    nodes.forEach((node, index) => {
      const sweep = (rangeEnd - rangeStart) * node.seconds / total;
      const gap = Math.min(.7, sweep / 8);
      const start = cursor + gap;
      const end = cursor + sweep - gap;
      const nodePath = [...path, node.label];
      if (end > start) {
        slices.push({
          node,
          start,
          end,
          inner: 39 + level * ringWidth,
          outer: 39 + (level + 1) * ringWidth - 1.5,
          color: categoryColour(node, index),
          path: nodePath,
        });
      }
      if (node.children.length) walk(node.children, cursor, cursor + sweep, level + 1, nodePath);
      cursor += sweep;
    });
  }

  walk(categories, 0, 360, 0, []);
  return slices;
}

function polar(radius: number, angle: number) {
  const radians = (angle - 90) * Math.PI / 180;
  return { x: 100 + radius * Math.cos(radians), y: 100 + radius * Math.sin(radians) };
}

function arcPath(slice: Slice): string {
  const start = polar(slice.outer, slice.start);
  const end = polar(slice.outer, slice.end);
  const innerEnd = polar(slice.inner, slice.end);
  const innerStart = polar(slice.inner, slice.start);
  const large = slice.end - slice.start > 180 ? 1 : 0;
  return `M ${start.x} ${start.y} A ${slice.outer} ${slice.outer} 0 ${large} 1 ${end.x} ${end.y} L ${innerEnd.x} ${innerEnd.y} A ${slice.inner} ${slice.inner} 0 ${large} 0 ${innerStart.x} ${innerStart.y} Z`;
}

function CategorySunburst({ categories, emptyMessage }: Props) {
  const [rootId, setRootId] = useState<string | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const root = rootId ? findCategory(categories, rootId) : null;
  const visible = root?.children.length ? root.children : categories;
  const slices = useMemo(() => createSlices(visible), [visible]);
  const total = root?.seconds ?? sumActivity(categories);
  const active = activeId ? findCategory(visible, activeId) : null;

  return (
    <section className="wc-fleet-section wc-fleet-sunburst-section" aria-label="Category Sunburst">
      <h3>Category Sunburst</h3>
      <div className="wc-fleet-sunburst-wrap">
        <svg className="wc-fleet-sunburst" viewBox="0 0 200 200" role="img" aria-label={`Category sunburst with ${formatActivityDuration(total)} total activity`}>
          {slices.map((slice) => (
            <path
              key={slice.node.id}
              d={arcPath(slice)}
              fill={slice.color}
              tabIndex={0}
              role="button"
              aria-label={`${slice.path.join(" > ")}: ${formatActivityDuration(slice.node.seconds)}`}
              className={activeId === slice.node.id ? "is-active" : ""}
              onMouseEnter={() => setActiveId(slice.node.id)}
              onMouseLeave={() => setActiveId(null)}
              onFocus={() => setActiveId(slice.node.id)}
              onBlur={() => setActiveId(null)}
              onClick={() => slice.node.children.length && setRootId(slice.node.id)}
              onKeyDown={(event) => {
                if ((event.key === "Enter" || event.key === " ") && slice.node.children.length) {
                  event.preventDefault();
                  setRootId(slice.node.id);
                }
              }}
            />
          ))}
          <circle cx="100" cy="100" r="35" className="wc-fleet-sunburst-hole" />
          <text x="100" y="96" textAnchor="middle" className="wc-fleet-sunburst-total">{formatActivityDuration(active?.seconds ?? total)}</text>
          <text x="100" y="112" textAnchor="middle" className="wc-fleet-sunburst-caption">{active?.label ?? root?.label ?? "tracked"}</text>
        </svg>
        {slices.length === 0 && <p className="wc-fleet-chart-empty">{emptyMessage}</p>}
      </div>
      <div className="wc-fleet-sunburst-info" aria-live="polite">
        {root && <button type="button" onClick={() => { setRootId(null); setActiveId(null); }}>← All categories</button>}
        {active && <span>{active.label} · {formatActivityDuration(active.seconds)} · {Math.round(active.seconds / total * 100)}%</span>}
      </div>
    </section>
  );
}

export function ActivityCategoryCharts({ categories, emptyMessage }: Props) {
  const [showPercent, setShowPercent] = useState(false);
  const total = sumActivity(categories);
  const topCategories = useMemo(() => flattenCategoryActivity(categories), [categories]);

  return (
    <section className="wc-fleet-category-section" aria-label="Category breakdown">
      <div className="wc-fleet-category-grid">
        <ActivityRankedList title="Top Categories" items={topCategories} emptyMessage={emptyMessage} />
        <section className="wc-fleet-section" aria-label="Category Tree">
          <h3>Category Tree</h3>
          <CategoryTree categories={categories} total={total} showPercent={showPercent} />
          <label className="wc-fleet-percent-toggle">
            <input type="checkbox" checked={showPercent} onChange={(event) => setShowPercent(event.target.checked)} />
            Show percent
          </label>
        </section>
        <CategorySunburst categories={categories} emptyMessage={emptyMessage} />
      </div>
    </section>
  );
}
