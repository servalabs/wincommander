import { useState, type ReactNode } from "react";
import { Icon } from "../../components/ui/icon";
import type { CleanupCategory } from "./cleanupCategories";
import "./PrivacyTraceWorkspace.css";

interface Props {
  accountLabel: string;
  categories: CleanupCategory[];
  getState: (category: CleanupCategory) => { count: number; items: string[]; loading: boolean; error?: string };
  renderCategory: (category: CleanupCategory) => ReactNode;
}

export default function PrivacyTraceWorkspace({ accountLabel, categories, getState, renderCategory }: Props) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [visibleCount, setVisibleCount] = useState(100);
  const selected = categories.find(category => category.id === selectedId) ?? categories[0];
  if (!selected) return null;
  const state = getState(selected);
  const visibleRecords = state.items.filter((item) => item.toLowerCase().includes(query.trim().toLowerCase()));
  const scopeLabel = selected.scopeAware ? accountLabel : selected.systemWide ? "Machine scope — unaffected by account selection" : "Fixed operation scope — unaffected by account selection";

  return <div className="privacy-trace-workspace">
    <nav aria-label="Privacy cleanup categories" className="privacy-trace-navigation">
      {categories.map(category => {
        const data = getState(category);
        return <button type="button" key={category.id} aria-current={selected.id === category.id ? "page" : undefined} onClick={() => { setSelectedId(category.id); setQuery(""); setVisibleCount(100); }}><Icon icon={category.icon} size={16} /><span><strong>{category.label}</strong><small>{category.scopeAware ? accountLabel : category.systemWide ? "Machine scope" : "Fixed operation scope"}</small></span><small>{data.loading ? "Scanning" : data.error ? "Failed" : data.count < 0 ? "Not scanned" : data.count.toLocaleString()}</small></button>;
      })}
    </nav>
    <section className="privacy-trace-detail" aria-label={`${selected.label} review`}>
      <header><h3>{selected.label}</h3><p>{selected.description}</p></header>
      <p className="privacy-trace-scope"><Icon icon={selected.scopeAware ? "user" : "info-sign"} size={13} /> {scopeLabel}</p>
      <div className="privacy-trace-action">{renderCategory(selected)}</div>
      {state.error && <p role="alert" className="privacy-trace-error">The scan did not complete. Review the error before taking action.</p>}
      {state.count < 0 && !state.loading && <p className="privacy-trace-notice">Scan this category to review the records Windows currently exposes.</p>}
      {state.items.length > 0 && <div className="privacy-trace-records"><h4>Observed records · {state.items.length.toLocaleString()} available preview entries</h4><input className="privacy-trace-search" aria-label={`Search ${selected.label} preview records`} placeholder="Search available records" value={query} onChange={(event) => { setQuery(event.target.value); setVisibleCount(100); }} /><ul>{visibleRecords.slice(0, visibleCount).map((item, index) => <li key={index}>{item}</li>)}</ul>{!visibleRecords.length && <p className="privacy-trace-notice">No preview records match this search.</p>}{visibleRecords.length > visibleCount && <button className="privacy-trace-more" type="button" onClick={() => setVisibleCount((count) => count + 100)}>Show more records ({visibleRecords.length - visibleCount} remaining)</button>}</div>}
      {state.count > state.items.length && <p className="privacy-trace-notice">The backend reports {state.count.toLocaleString()} records but returns {state.items.length.toLocaleString()} preview entries. This is an incomplete preview. Clearing applies to the entire supported category; individual record selection is unavailable.</p>}
      <p className="privacy-trace-notice">Privacy records are not a disk-space estimate. Clearing a category can remove history or settings; review its confirmation and account scope.</p>
    </section>
  </div>;
}
