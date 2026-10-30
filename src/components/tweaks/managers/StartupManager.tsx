import { useEffect, useState, useCallback, useMemo, useRef } from "react";
import { Button, Tag, Spinner, InputGroup } from "@/components/ui/bp";
import SectionCard from "../../shared/SectionCard";
import { executeBackendCommand, type StartupImpactEntry } from "../../../hooks/useBackend";
import { showSuccess, showError } from "../../../utils/toast";
import { enabledRowsFirst } from "./systemManagerSort";
import { recordMaintenanceOperation } from "../../../hooks/useMaintenanceHistory";
import { useAppConfirm } from "../../shared/AppConfirmDialog";
import { matchesStartupSearch, startupIdentity, startupSignature, type StartupItem } from "./startupReview";
import { StartupReviewRow } from "./StartupReviewRow";
import "./SystemManagers.css";
import "./StartupReview.css";

export default function StartupManager({ embedded = false, scanKey = 0, reviewEntries = [] }: { embedded?: boolean; scanKey?: number; reviewEntries?: StartupImpactEntry[] }) {
  const [isOpen, setIsOpen] = useState(false);
  const [items, setItems] = useState<StartupItem[]>([]);
  const [loading, setLoading] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string>();
  const [pending, setPending] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState("");
  const [statusFilter, setStatusFilter] = useState("all");
  const embeddedScanKey = useRef<number | undefined>(undefined);
  const requestConfirm = useAppConfirm();

  const refresh = useCallback(async () => {
    setLoading(true); setError(undefined);
    try {
      const res = await executeBackendCommand<StartupItem[] | { error?: boolean; message?: string }>("Get-StartupItems");
      if (res.success && Array.isArray(res.data)) setItems(res.data);
      else throw new Error(res.error || (!Array.isArray(res.data) && res.data?.message) || "Failed to load startup items");
    } catch (cause) { setError(String(cause)); }
    finally { setLoading(false); setLoaded(true); }
  }, []);

  useEffect(() => {
    if (embedded) {
      if (embeddedScanKey.current !== scanKey) { embeddedScanKey.current = scanKey; void refresh(); }
    } else if (isOpen && !loaded && !loading) void refresh();
  }, [embedded, isOpen, loaded, loading, refresh, scanKey]);

  const handleToggle = useCallback(async (item: StartupItem) => {
    if (pending.size || loading) return;
    if (item.IsEnabled && !await requestConfirm({ title: `Disable ${item.Name} at startup?`, description: "The application stays installed. Its automatic syncing, updates or notifications may stop until you open it.", confirmLabel: "Disable at startup", destructive: false })) return;
    const identity = startupIdentity(item);
    setPending(new Set([identity]));
    try {
      const res = await executeBackendCommand<{ success?: boolean; message?: string }>(item.IsEnabled ? "Disable-StartupItem" : "Enable-StartupItem", { Name: item.Name, Location: item.Location });
      if (!res.success || res.data?.success !== true) throw new Error(res.error || res.data?.message || "Failed to change startup entry");
      recordMaintenanceOperation({ tool: "startup", title: item.IsEnabled ? "Disable startup entry" : "Enable startup entry", outcome: "completed", affectedItems: 1 });
      showSuccess(`${item.IsEnabled ? "Disabled" : "Enabled"} ${item.Name}`);
      await refresh();
    } catch (cause) {
      recordMaintenanceOperation({ tool: "startup", title: "Change startup entry", outcome: "failed", affectedItems: 0 });
      showError(String(cause));
    } finally { setPending(new Set()); }
  }, [loading, pending.size, refresh, requestConfirm]);

  const filtered = useMemo(() => enabledRowsFirst(items.filter(item => (
    statusFilter === "all" || (statusFilter === "enabled" && item.IsEnabled) || (statusFilter === "disabled" && !item.IsEnabled)
    || (statusFilter === "review" && item.Recommendation === "Disable"))
    && matchesStartupSearch(item, startupSignature(item, reviewEntries), filter)), item => item.IsEnabled), [items, filter, statusFilter, reviewEntries]);
  const enabled = items.filter(item => item.IsEnabled).length;
  const isResultsLoading = loading && !items.length;
  const isResultsEmpty = !loading && !error && !filtered.length;
  const headerRight = <div className="system-manager-actions"><Tag minimal>{items.length} entries</Tag>{!embedded && <Button minimal icon="refresh" aria-label="Refresh startup items" onClick={event => { event.stopPropagation(); void refresh(); }} loading={loading} disabled={!!pending.size} />}</div>;
  const body = <>
    <div className="startup-review-summary"><div><strong>{items.length}</strong><span>Startup entries</span></div><div><strong>{enabled}</strong><span>Enabled at sign-in</span></div><div><strong>{items.length - enabled}</strong><span>Disabled entries</span></div></div>
    <p className="startup-review-intro">Review what launches when you sign in. Expand an entry for its purpose, publisher and location.</p>
    <div className="startup-review-toolbar">
      <div className="startup-review-filters" role="group" aria-label="Filter startup entries">{[["all", "All"], ["enabled", "Enabled"], ["disabled", "Disabled"], ["review", "Review suggested"]].map(([value, label]) => <button type="button" key={value} aria-pressed={statusFilter === value} onClick={() => setStatusFilter(value)}>{label}</button>)}</div>
      <InputGroup aria-label="Search startup items" placeholder="Search names, publishers or paths…" leftIcon="search" value={filter} onChange={event => setFilter(event.currentTarget.value)} />
    </div>
    {error && <p role="alert" className="startup-review-error">{error} <button type="button" onClick={() => void refresh()}>Try again</button></p>}
    <div className={`startup-review-results${isResultsLoading ? " is-loading" : ""}${isResultsEmpty ? " is-empty" : ""}`} aria-busy={loading}>
      {isResultsLoading && <Spinner size={20} />}
      {isResultsEmpty && <p className="system-manager-empty">No startup entries match.</p>}
      {filtered.map(item => <StartupReviewRow key={startupIdentity(item)} item={item} signature={startupSignature(item, reviewEntries)} pending={pending.has(startupIdentity(item))} busy={loading || !!pending.size} onToggle={() => void handleToggle(item)} />)}
    </div>
    <p className="startup-review-footnote">{filtered.length} of {items.length} entries shown · Registry and Startup folders. Services and scheduled tasks have their own views. Removal is not supported here; use Disable to retain the entry.</p>
  </>;
  if (embedded) return <div className="system-manager-embedded"><div className="system-manager-embedded__meta">{headerRight}</div>{body}</div>;
  return <SectionCard title="Startup Manager" icon="play" collapsible isOpen={isOpen} onToggle={() => setIsOpen(value => !value)} headerRight={headerRight}>{body}</SectionCard>;
}
