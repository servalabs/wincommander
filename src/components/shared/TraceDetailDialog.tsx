import { useEffect, useMemo, useState } from "react";
import { Button, Icon } from "@/components/ui/bp";
import type { CleanupCategory } from "../../panels/cleanup/cleanupCategories";
import {
  buildTraceView,
  formatTraceCell,
  paginateTraceRows,
  traceCellText,
  traceRowsToTsv,
  visibleTraceColumns,
  type TraceDataset,
  type TraceTableRow,
} from "./traceTable";
import { showError, showSuccess } from "../../utils/toast";
import "./TraceDetailDialog.css";

interface TraceDetailDialogProps {
  category: Pick<CleanupCategory,
    "label" | "description" | "icon" | "severity" | "scopeAware" | "systemWide" |
    "id" | "schedulable" | "minIntervalMinutes" | "schedulerRunAsSystem" | "regeneratesNote" |
    "clearDataKey"
  >;
  isOpen: boolean;
  count: number;
  items: string[];
  rawData?: unknown;
  groupedItems?: Array<{ title: string; count: number; items: string[]; rawData?: unknown }>;
  clearing: boolean;
  onClose: () => void;
  onClear?: () => void;
  clearDisabled?: boolean;
  clearLabel?: string;
}

export default function TraceDetailDialog({
  category,
  isOpen,
  count,
  items,
  rawData,
  groupedItems,
  clearing,
  onClose,
  onClear,
  clearDisabled,
  clearLabel = "Clear",
}: TraceDetailDialogProps) {
  const [query, setQuery] = useState("");
  const view = useMemo(() => buildTraceView(rawData, items), [rawData, items]);
  const isPreviewTruncated = !!(
    rawData &&
    typeof rawData === "object" &&
    "truncated" in rawData &&
    (rawData as { truncated?: unknown }).truncated === true
  );
  const previewLimit = isPreviewTruncated && rawData && typeof rawData === "object" && "previewLimit" in rawData
    ? (rawData as { previewLimit?: unknown }).previewLimit
    : null;
  const groupedViews = useMemo(
    () => (groupedItems ?? []).map((group) => ({
      ...group,
      view: buildTraceView(group.rawData, group.items),
    })),
    [groupedItems],
  );

  if (!isOpen) {
    return null;
  }

  const datasetCount = groupedViews.length
    ? groupedViews.reduce((total, group) => total + group.view.datasets.length, 0)
    : view.datasets.length;

  return (
    <div className="trace-dialog" role="dialog" aria-modal="true" aria-label={category.label}>
      <div className="trace-dialog__panel">
        <header className="trace-dialog__header">
          <div className={`trace-dialog__icon trace-dialog__icon--${category.severity}`}>
            <Icon icon={category.icon as never} size={18} />
          </div>
          <div>
            <h2>{category.label}</h2>
            <p>{category.description}</p>
          </div>
          <div className="trace-dialog__actions">
            <Button icon="cross" minimal aria-label={`Dismiss ${category.label} details`} onClick={onClose} />
          </div>
        </header>
        <div className="trace-dialog__toolbar">
          <div className="trace-dialog__summary">
            <strong>{count}</strong>
            <span>{count === 1 ? "item" : "items"}</span>
            {datasetCount > 0 ? <small>{datasetCount} {datasetCount === 1 ? "dataset" : "datasets"}</small> : null}
          </div>
          <label className="trace-dialog__search">
            <Icon icon="search" size={13} />
            <input
              value={query}
              onChange={(event) => setQuery(event.currentTarget.value)}
              placeholder="Filter every field"
              aria-label={`Filter ${category.label} details`}
            />
            {query ? (
              <button type="button" onClick={() => setQuery("")} aria-label={`Clear ${category.label} filter`}>
                <Icon icon="cross" size={11} />
              </button>
            ) : null}
          </label>
        </div>
        {isPreviewTruncated ? (
          <p className="trace-dialog__bounded-preview" role="status">
            Showing the first {typeof previewLimit === "number" ? previewLimit : items.length} entries. The total above includes every item found.
          </p>
        ) : null}
        <div className="trace-dialog__items">
          {view.metadata.length > 0 && groupedViews.length === 0 ? (
            <section className="trace-dialog__metadata" aria-label="Artifact summary">
              {view.metadata.map((item) => (
                <div key={item.label}>
                  <span>{item.label}</span>
                  <strong title={traceCellText(item.value)}>{traceCellText(item.value)}</strong>
                </div>
              ))}
            </section>
          ) : null}
          {groupedViews.length ? (
            groupedViews.map((group) => (
              <section className="trace-dialog__group" key={`${group.title}:${group.count}:${group.items[0] ?? "empty"}`}>
                <div className="trace-dialog__group-header">
                  <strong>{group.title}</strong>
                  <span>{group.count} {group.count === 1 ? "item" : "items"}</span>
                </div>
                <div className="trace-dialog__group-items">
                  {group.view.datasets.length ? (
                    group.view.datasets.map((dataset) => (
              <TraceDataTable key={`${group.title}:${dataset.id}`} dataset={dataset} query={query} isEventLog={category.id === "eventLogs"} />
                    ))
                  ) : (
                    <span className="trace-dialog__empty">No entries found for this user.</span>
                  )}
                </div>
              </section>
            ))
          ) : view.datasets.length ? (
            view.datasets.map((dataset) => (
              <TraceDataTable key={dataset.id} dataset={dataset} query={query} isEventLog={category.id === "eventLogs"} />
            ))
          ) : (
            <span className="trace-dialog__empty">No entries found.</span>
          )}
        </div>
        <footer className="trace-dialog__footer">
          <Button text="Close" minimal aria-label={`Close ${category.label} details`} onClick={onClose} />
          {onClear ? (
            <Button
              text={clearLabel}
              intent="danger"
              loading={clearing}
              disabled={clearDisabled || clearing}
              aria-label={`${clearLabel} ${category.label}`}
              onClick={onClear}
            />
          ) : null}
        </footer>
      </div>
    </div>
  );
}

function compareCells(left: unknown, right: unknown): number {
  if (typeof left === "number" && typeof right === "number") return left - right;
  return traceCellText(left as never).localeCompare(traceCellText(right as never), undefined, {
    numeric: true,
    sensitivity: "base",
  });
}

function TracePagination({
  dataset,
  pageView,
  matchingRows,
  setPage,
}: {
  dataset: TraceDataset;
  pageView: ReturnType<typeof paginateTraceRows>;
  matchingRows: unknown[];
  setPage: React.Dispatch<React.SetStateAction<number>>;
}) {
  if (pageView.totalPages <= 1) return null;
  return (
    <div className="trace-dialog__pagination" aria-label={`${dataset.title} pages`}>
      <span>Rows {pageView.startIndex + 1}–{pageView.startIndex + pageView.rows.length} of {matchingRows.length}</span>
      <div>
        <button type="button" onClick={() => setPage((current) => Math.max(0, current - 1))} disabled={pageView.page === 0} aria-label={`Previous ${dataset.title} page`}><Icon icon="chevron-left" size={11} /></button>
        <strong>{pageView.page + 1} / {pageView.totalPages}</strong>
        <button type="button" onClick={() => setPage((current) => Math.min(pageView.totalPages - 1, current + 1))} disabled={pageView.page === pageView.totalPages - 1} aria-label={`Next ${dataset.title} page`}><Icon icon="chevron-right" size={11} /></button>
      </div>
    </div>
  );
}

function isFileSystemPath(value: string): boolean {
  return /^(?:[a-z]:\\|\\\\)/i.test(value.trim());
}

/** Keeps the full path available for the title/copy action while making the
 * meaningful Windows prefix immediately recognisable in dense cleanup tables. */
function CompactTracePath({ path }: { path: string }) {
  const normalized = path.trim();
  const driveMatch = /^([A-Za-z]:)(\\.*)?$/.exec(normalized);
  if (!driveMatch) return <>{normalized}</>;

  const [, drive, remainder = ""] = driveMatch;
  const isWindows = /^\\Windows(?:\\|$)/i.test(remainder);
  const isProgramFiles = /^\\Program Files(?: \(x86\))?(?:\\|$)/i.test(remainder);
  const suffix = isWindows
    ? remainder.replace(/^\\Windows/i, "")
    : isProgramFiles
      ? remainder.replace(/^\\Program Files(?: \(x86\))?/i, "")
      : remainder;

  return (
    <span className="trace-dialog__compact-path">
      <span className="trace-dialog__path-drive" title={`${drive} drive`}>{drive}</span>
      {isWindows ? <span className="trace-dialog__path-root"><Icon icon="desktop" size={11} />Windows</span> : null}
      {isProgramFiles ? <span className="trace-dialog__path-root"><Icon icon="folder-shared" size={11} />PF</span> : null}
      {suffix ? <span className="trace-dialog__path-suffix">{suffix}</span> : null}
    </span>
  );
}

function TraceDataTable({ dataset, query, isEventLog }: { dataset: TraceDataset; query: string; isEventLog: boolean }) {
  const [sortColumn, setSortColumn] = useState<string | null>(null);
  const [sortDirection, setSortDirection] = useState<"asc" | "desc">("asc");
  const [page, setPage] = useState(0);
  const [copyState, setCopyState] = useState<"idle" | "copying" | "copied">("idle");

  const matchingRows = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    let rows = dataset.rows.map((row, sourceIndex) => ({ row, sourceIndex }));
    rows = needle
      ? rows.filter(({ row }) =>
          Object.values(row).some((value) => traceCellText(value).toLocaleLowerCase().includes(needle)),
        )
      : rows;

    if (sortColumn) {
      rows = [...rows].sort((left, right) => {
        const order = compareCells(left.row[sortColumn], right.row[sortColumn]);
        return sortDirection === "asc" ? order : -order;
      });
    }
    return rows;
  }, [dataset.rows, query, sortColumn, sortDirection]);
  const pageView = useMemo(() => paginateTraceRows(matchingRows, page), [matchingRows, page]);
  const columns = useMemo(
    () => visibleTraceColumns(dataset.columns, matchingRows.map(({ row }) => row)),
    [dataset.columns, matchingRows],
  );

  useEffect(() => {
    setPage(0);
    setCopyState("idle");
  }, [dataset.id, dataset.rows, query]);

  const toggleSort = (column: string) => {
    if (sortColumn === column) {
      setSortDirection((current) => current === "asc" ? "desc" : "asc");
    } else {
      setSortColumn(column);
      setSortDirection("asc");
    }
    setPage(0);
    setCopyState("idle");
  };

  const copyRows = async () => {
    setCopyState("copying");
    try {
      if (!navigator.clipboard?.writeText) throw new Error("Clipboard access is unavailable");
      const tsv = traceRowsToTsv(columns, matchingRows.map(({ row }) => row));
      await navigator.clipboard.writeText(tsv);
      setCopyState("copied");
      showSuccess(`${matchingRows.length} ${matchingRows.length === 1 ? "row" : "rows"} copied as TSV.`);
    } catch (error) {
      setCopyState("idle");
      showError(`Couldn't copy trace rows: ${error}`);
    }
  };

  const copyValue = async (value: string) => {
    try {
      await navigator.clipboard.writeText(value);
      showSuccess("Location copied.");
    } catch (error) {
      showError(`Couldn't copy location: ${error}`);
    }
  };

  return (
    <section className="trace-dialog__dataset">
      <div className="trace-dialog__dataset-header">
        <strong>{dataset.title}</strong>
        <div className="trace-dialog__dataset-actions">
          <span>{matchingRows.length === dataset.rows.length ? `${dataset.rows.length} rows` : `${matchingRows.length} of ${dataset.rows.length} rows`}</span>
          <Button
            icon={copyState === "copied" ? "tick" : "duplicate"}
            text={copyState === "copied" ? "Copied" : "Copy TSV"}
            minimal
            small
            loading={copyState === "copying"}
            disabled={matchingRows.length === 0 || copyState === "copying"}
            aria-label={`Copy ${dataset.title} as TSV`}
            onClick={() => void copyRows()}
          />
        </div>
      </div>
      {matchingRows.length ? (
        <>
          <TracePagination dataset={dataset} pageView={pageView} matchingRows={matchingRows} setPage={setPage} />
          <div className="trace-dialog__table-scroll" aria-label={`${dataset.title} table. Scroll horizontally to see more columns.`}>
            <table>
            <thead>
              <tr>
                <th className="trace-dialog__row-number" scope="col">#</th>
                {columns.map((column) => (
                  <th key={column} scope="col" aria-sort={sortColumn === column ? (sortDirection === "asc" ? "ascending" : "descending") : "none"}>
                    <button
                      type="button"
                      aria-label={`Sort ${dataset.title} by ${column}`}
                      onClick={() => toggleSort(column)}
                    >
                      <span>{column}</span>
                      {sortColumn === column ? <Icon icon={sortDirection === "asc" ? "chevron-up" : "chevron-down"} size={10} /> : null}
                    </button>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {pageView.rows.map(({ row, sourceIndex }: { row: TraceTableRow; sourceIndex: number }, rowIndex) => (
                <tr key={`${dataset.id}:${sourceIndex}`}>
                  <td className="trace-dialog__row-number">{pageView.startIndex + rowIndex + 1}</td>
                  {columns.map((column) => {
                    const text = formatTraceCell(row[column], column);
                    const isPath = isFileSystemPath(text);
                    const isLogChannel = isEventLog && /(?:log|channel)/i.test(column) && text !== "—";
                    return (
                      <td key={column} title={text}>
                        <div className="trace-dialog__cell-value">
                          <span>{isPath ? <CompactTracePath path={text} /> : text}</span>
                          {(isPath || isLogChannel) && (
                            <span className="trace-dialog__cell-actions">
                              <button type="button" onClick={() => void copyValue(text)} aria-label={`Copy ${column}`}>Copy</button>
                            </span>
                          )}
                        </div>
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
            </table>
          </div>
        </>
      ) : (
        <span className="trace-dialog__empty">No rows match this filter.</span>
      )}
    </section>
  );
}
