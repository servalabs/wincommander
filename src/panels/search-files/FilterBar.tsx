// src/panels/search-files/FilterBar.tsx
//
// Refinement chips under the search box: multi-select type filters
// (Files/Folders are mutually exclusive; extension categories combine
// freely) plus Size and Modified selects. Pure renderer.

import { Icon } from "@/components/ui/icon";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { DateFilter, SearchType, SizeFilter } from "@/lib/fileNameSearch";

const TYPE_CHIPS: [SearchType, string, string][] = [
  ["files", "document", "Files"],
  ["folders", "folder-close", "Folders"],
  ["documents", "document", "Docs"],
  ["images", "media", "Images"],
  ["videos", "video", "Videos"],
  ["audio", "music", "Audio"],
  ["archives", "compressed", "Archives"],
  ["apps", "application", "Apps"],
  ["code", "code", "Code"],
];

interface FilterBarProps {
  searchTypes: Set<SearchType>;
  sizeFilter: SizeFilter;
  dateFilter: DateFilter;
  onToggleType: (t: SearchType) => void;
  onClearTypes: () => void;
  onSizeChange: (s: SizeFilter) => void;
  onDateChange: (d: DateFilter) => void;
}

const chipClass = (active: boolean) =>
  `inline-flex items-center gap-1.5 rounded-[var(--radius-full)] border px-2.5 py-1 text-[11px] font-medium transition-colors duration-150 ${
    active
      ? "border-[var(--accent-line)] bg-[var(--accent-soft)] text-[var(--accent)]"
      : "border-[var(--border)] bg-[var(--surface-2)] text-[var(--text-dim)] hover:text-[var(--text)]"
  }`;

export default function FilterBar({
  searchTypes,
  sizeFilter,
  dateFilter,
  onToggleType,
  onClearTypes,
  onSizeChange,
  onDateChange,
}: FilterBarProps) {
  return (
    <div className="search-filter-row" aria-label="Search filters">
      <div className="search-filter-types" role="group" aria-label="File type (multi-select)">
        {/* "All" is special — clears the selection. Every other chip is a
            multi-select toggle. */}
        <button
          type="button"
          title="All — clear type filters"
          aria-pressed={searchTypes.size === 0}
          onClick={onClearTypes}
          className={chipClass(searchTypes.size === 0)}
        >
          <Icon icon="th" size={13} /> All
        </button>
        {TYPE_CHIPS.map(([val, ic, lbl]) => {
          const active = searchTypes.has(val);
          return (
            <button
              key={val}
              type="button"
              title={`${lbl}${active ? " — click to remove" : " — click to add to filter"}`}
              aria-pressed={active}
              onClick={() => onToggleType(val)}
              className={chipClass(active)}
            >
              <Icon icon={ic} size={13} /> {lbl}
            </button>
          );
        })}
      </div>
      <div className="search-filter-field">
        <span id="sfp-size-label">Size</span>
        <Select value={sizeFilter} onValueChange={(v) => onSizeChange(v as SizeFilter)}>
          <SelectTrigger className="search-filter-select-trigger" aria-labelledby="sfp-size-label"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="any">Any</SelectItem>
            <SelectItem value="tiny">Under 1 MB</SelectItem>
            <SelectItem value="medium">1-100 MB</SelectItem>
            <SelectItem value="large">Over 100 MB</SelectItem>
            <SelectItem value="huge">Over 1 GB</SelectItem>
          </SelectContent>
        </Select>
      </div>
      <div className="search-filter-field">
        <span id="sfp-modified-label">Modified</span>
        <Select value={dateFilter} onValueChange={(v) => onDateChange(v as DateFilter)}>
          <SelectTrigger className="search-filter-select-trigger" aria-labelledby="sfp-modified-label"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="any">Any time</SelectItem>
            <SelectItem value="today">Today</SelectItem>
            <SelectItem value="week">This week</SelectItem>
            <SelectItem value="month">This month</SelectItem>
          </SelectContent>
        </Select>
      </div>
    </div>
  );
}
