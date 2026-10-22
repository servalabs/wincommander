import { useState } from "react";
import { Icon, Spinner } from "@/components/ui/bp";
import { toExplorerDisplayPath } from "@/lib/windowsPathDisplay";
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

export function formatSize(sizeKB: number | undefined) {
  if (sizeKB === undefined || !Number.isFinite(sizeKB) || sizeKB <= 0) return "Size unavailable";
  return sizeKB >= 1024 * 1024 ? `${(sizeKB / 1024 / 1024).toFixed(1)} GiB` : sizeKB >= 1024 ? `${(sizeKB / 1024).toFixed(1)} MiB` : `${sizeKB.toFixed(0)} KiB`;
}

export default function ApplicationReviewResults(props: Props) {
  const [category, setCategory] = useState<string | null>(null);
  const [detailId, setDetailId] = useState<string | null>(null);
  const activeCategory = props.groups.some(([name]) => name === category) ? category : null;
  const groups = activeCategory ? props.groups.filter(([name]) => name === activeCategory) : props.groups;
  const allItems = props.groups.flatMap(([, items]) => items);
  const detail = allItems.find(item => item.id === detailId);
  const detailLocation = toExplorerDisplayPath(detail?.installLocation);

  return <div className="application-review-workspace">
    <nav className="application-review-categories" aria-label="Application categories">
      <button type="button" aria-current={!activeCategory ? "page" : undefined} onClick={() => setCategory(null)}><Icon icon="applications" /><span>All applications</span><strong>{allItems.length}</strong></button>
      {props.groups.map(([name, items]) => <button type="button" key={name} aria-current={activeCategory === name ? "page" : undefined} onClick={() => setCategory(name)}><Icon icon={name === "Programs" ? "desktop" : "box"} /><span>{name}</span><strong>{items.length}</strong></button>)}
      {!props.bcuInstalled && <button type="button" disabled={props.bcuInstalling || props.removing} onClick={props.onInstallBcu}><Icon icon="download" /><span>{props.bcuInstalling ? "Installing engine…" : "Install program inventory engine"}</span></button>}
    </nav>
    <div className="application-review-main">
      <div className="application-review-results" aria-busy={props.loading}>
      {props.errors.store && <p role="alert" className="application-review-error">Store apps: {props.errors.store}</p>}
      {props.errors.programs && <p role="alert" className="application-review-error">Programs: {props.errors.programs}</p>}
      {props.loading && !allItems.length && <div className="application-review-empty"><Spinner size={20} /><p>Reading installed applications…</p></div>}
      {!props.loading && !allItems.length && <p className="application-review-empty">No applications match the current filters.</p>}
      {groups.map(([name, items]) => <section key={name} aria-label={name}>
        <header className="application-review-group"><strong>{name}</strong><button type="button" disabled={props.removing} onClick={() => props.onSelectGroup(name)}>Select category</button></header>
        {items.map(item => <div key={item.id} className={`application-review-row${props.selected.has(item.id) ? " is-selected" : ""}${detailId === item.id ? " is-inspected" : ""}`}>
          <input type="checkbox" checked={props.selected.has(item.id)} onChange={() => { props.onToggle(item.id); setDetailId(item.id); }} disabled={props.removing || !!item.removalUnavailable} aria-label={`Select ${item.label} for removal`} title={item.removalUnavailable} />
          <button type="button" className="application-review-item" aria-expanded={detailId === item.id} aria-controls={detailId === item.id ? "application-review-detail" : undefined} onClick={() => setDetailId(detailId === item.id ? null : item.id)}>
            <AppIcon id={item.id} category={item.category} iconData={item.iconData} size={30} preferNative />
            <span><strong>{item.label}</strong><small>{item.publisher || (item.source === "program" ? "Desktop application" : item.source === "store" ? "Store package" : "Windows component")}{item.version ? ` · ${item.version}` : ""}{item.removalUnavailable ? " · Unavailable" : item.riskNote ? " · Review impact" : ""}</small></span>
            <span className="application-review-size">{formatSize(item.sizeKB)}</span><Icon icon="chevron-right" size={14} />
          </button>
        </div>)}
      </section>)}
      </div>
      {detail && <aside id="application-review-detail" className="application-review-detail" aria-label={`Details for ${detail.label}`}>
        <div className="application-review-detail-heading"><AppIcon id={detail.id} category={detail.category} iconData={detail.iconData} size={30} preferNative /><strong>{detail.label}</strong><button type="button" aria-label="Close application details" onClick={() => setDetailId(null)}><Icon icon="cross" size={16} /></button></div>
        <dl className="application-review-metadata">
          <div><dt>Publisher</dt><dd>{detail.publisher || "Unavailable"}</dd></div>
          <div><dt>Version</dt><dd>{detail.version || "Unavailable"}</dd></div>
          <div><dt>Reported size</dt><dd>{detail.sizeKB && Number.isFinite(detail.sizeKB) && detail.sizeKB > 0 ? formatSize(detail.sizeKB) : "Unavailable"}</dd></div>
          <div><dt>Type</dt><dd>{detail.source === "program" ? "Desktop application" : detail.source === "store" ? "Store package" : "Windows component"}</dd></div>
          {detail.installDate && <div><dt>Installed</dt><dd>{detail.installDate}</dd></div>}
          {detail.installLocation && <div className="application-review-metadata-wide"><dt>Location</dt><dd>{detailLocation ?? "Unavailable"}</dd></div>}
        </dl>
        {detail.riskNote && <p>{detail.riskNote}</p>}
        {detail.isProvisioned && <p>Windows also provisions this package for new user accounts. Removal may affect more than the current account.</p>}
        <div className="application-review-detail-actions"><span>{detail.removalUnavailable || (props.selected.has(detail.id) ? "Selected for removal — review and confirm below." : "Removal requires review and confirmation.")}</span><button type="button" disabled={props.removing || !!detail.removalUnavailable} onClick={() => props.onToggle(detail.id)}>{props.selected.has(detail.id) ? "Keep application" : "Select for removal"}</button></div>
        <details className="application-review-package-id"><summary>Package identifier</summary><p>{detail.packageFullName || detail.id}</p></details>
      </aside>}
    </div>
  </div>;
}
