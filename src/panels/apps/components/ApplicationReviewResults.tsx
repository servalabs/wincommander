import { useState } from "react";
import { Icon, Spinner } from "@/components/ui/bp";
import AppIcon from "./AppIcon";
import type { DebloatItem } from "../types";
import "./ApplicationReviewResults.css";

interface Props {
  groups: ReadonlyArray<readonly [string, DebloatItem[]]>;
  selected: Set<string>;
  removing: boolean;
  loading: boolean;
  bcuInstalled: boolean;
  bcuInstalling: boolean;
  errors: { store?: string | null; programs?: string | null };
  onToggle: (id: string) => void;
  onSelectGroup: (category: string) => void;
  onInstallBcu: () => void;
}

function formatSize(sizeKB: number | undefined) {
  if (sizeKB === undefined || !Number.isFinite(sizeKB) || sizeKB <= 0) return "Size unavailable";
  return sizeKB >= 1024 * 1024 ? `${(sizeKB / 1024 / 1024).toFixed(1)} GB` : `${(sizeKB / 1024).toFixed(1)} MB`;
}

export default function ApplicationReviewResults(props: Props) {
  const [category, setCategory] = useState<string | null>(null);
  const [detailId, setDetailId] = useState<string | null>(null);
  const activeCategory = props.groups.some(([name]) => name === category) ? category : null;
  const groups = activeCategory ? props.groups.filter(([name]) => name === activeCategory) : props.groups;
  const allItems = props.groups.flatMap(([, items]) => items);
  const detail = allItems.find(item => item.id === detailId);

  return <div className="application-review-workspace">
    <nav className="application-review-categories" aria-label="Application categories">
      <button type="button" aria-current={!activeCategory ? "page" : undefined} onClick={() => setCategory(null)}><Icon icon="applications" /><span>All applications</span><strong>{allItems.length}</strong></button>
      {props.groups.map(([name, items]) => <button type="button" key={name} aria-current={activeCategory === name ? "page" : undefined} onClick={() => setCategory(name)}><Icon icon={name === "Programs" ? "desktop" : "box"} /><span>{name}</span><strong>{items.length}</strong></button>)}
      {!props.bcuInstalled && <button type="button" disabled={props.bcuInstalling || props.removing} onClick={props.onInstallBcu}><Icon icon="download" /><span>{props.bcuInstalling ? "Installing engine…" : "Install program inventory engine"}</span></button>}
    </nav>
    <div className="application-review-results" aria-busy={props.loading}>
      {props.errors.store && <p role="alert" className="application-review-error">Store apps: {props.errors.store}</p>}
      {props.errors.programs && <p role="alert" className="application-review-error">Programs: {props.errors.programs}</p>}
      {props.loading && !allItems.length && <div className="application-review-empty"><Spinner size={20} /><p>Reading installed applications…</p></div>}
      {!props.loading && !allItems.length && <p className="application-review-empty">No applications match the current filters.</p>}
      {groups.map(([name, items]) => <section key={name} aria-label={name}>
        <header className="application-review-group"><strong>{name}</strong><button type="button" disabled={props.removing} onClick={() => props.onSelectGroup(name)}>Select category</button></header>
        {items.map(item => <div key={item.id} className={`application-review-row${props.selected.has(item.id) ? " is-selected" : ""}`}>
          <input type="checkbox" checked={props.selected.has(item.id)} onChange={() => props.onToggle(item.id)} disabled={props.removing} aria-label={`Select ${item.label} for removal`} />
          <button type="button" className="application-review-item" aria-expanded={detailId === item.id} onClick={() => setDetailId(detailId === item.id ? null : item.id)}>
            <AppIcon id={item.id} category={item.category} iconData={item.iconData} size={30} preferNative />
            <span><strong>{item.label}</strong><small>{item.source === "program" ? "Desktop application" : item.source === "store" ? "Microsoft Store application" : "Windows component"}{item.riskNote ? " · Review impact" : ""}</small></span>
            <span className="application-review-size">{formatSize(item.sizeKB)}</span><Icon icon="chevron-right" size={14} />
          </button>
        </div>)}
      </section>)}
      {detail && <aside className="application-review-detail" aria-label={`Details for ${detail.label}`}><div className="flex items-center justify-between gap-2"><strong>{detail.label}</strong><button type="button" aria-label="Close application details" onClick={() => setDetailId(null)}><Icon icon="cross" size={16} /></button></div><p>{detail.riskNote ?? "Review whether you use this application before removing it. Its uninstaller may remove settings or require a restart."}</p><p className="application-review-id">{detail.id}</p><p>Reported installed size: {formatSize(detail.sizeKB)}. Installed size is not a measurement of space recovered after removal.</p></aside>}
    </div>
  </div>;
}
