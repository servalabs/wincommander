import { useState, type ReactNode } from "react";
import { Icon } from "../../components/ui/icon";
import type { CleanupCategory } from "./cleanupCategories";
import "./PrivacyTraceWorkspace.css";

interface Props {
  categories: CleanupCategory[];
  getState: (category: CleanupCategory) => { count: number; items: string[]; loading: boolean; error?: string };
  renderCategory: (category: CleanupCategory) => ReactNode;
}

export default function PrivacyTraceWorkspace({ categories, getState, renderCategory }: Props) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const selected = categories.find(category => category.id === selectedId) ?? categories[0];
  if (!selected) return null;
  const state = getState(selected);

  return <div className="privacy-trace-workspace">
    <nav aria-label="Privacy cleanup categories" className="privacy-trace-navigation">
      {categories.map(category => {
        const data = getState(category);
        return <button type="button" key={category.id} aria-current={selected.id === category.id ? "page" : undefined} onClick={() => setSelectedId(category.id)}><Icon icon={category.icon} size={16} /><span><strong>{category.label}</strong><small>{category.scopeAware ? "Selected account" : category.systemWide ? "Machine scope" : "Fixed operation scope"}</small></span><small>{data.loading ? "Scanning" : data.error ? "Failed" : data.count < 0 ? "Not scanned" : data.count.toLocaleString()}</small></button>;
      })}
    </nav>
    <section className="privacy-trace-detail" aria-label={`${selected.label} review`}>
      <header><h3>{selected.label}</h3><p>{selected.description}</p></header>
      <div className="privacy-trace-action">{renderCategory(selected)}</div>
      {state.error && <p role="alert" className="privacy-trace-error">The scan did not complete. Review the error before taking action.</p>}
      {state.count < 0 && !state.loading && <p className="privacy-trace-notice">Scan this category to review the records Windows currently exposes.</p>}
      {state.items.length > 0 && <div className="privacy-trace-records"><h4>Observed records</h4><ul>{state.items.slice(0, 100).map((item, index) => <li key={index}>{item}</li>)}</ul>{state.items.length > 100 && <p className="privacy-trace-notice">Showing the first 100 preview records. Open details to inspect the full available result.</p>}</div>}
      <p className="privacy-trace-notice">Privacy records are not a disk-space estimate. Clearing a category can remove history or settings; review its confirmation and account scope.</p>
    </section>
  </div>;
}
