import { useId, useState } from "react";
import { Icon, Button, Tag } from "@/components/ui/bp";
import type { StartupImpactEntry } from "../../../hooks/useBackend";
import type { StartupItem } from "./startupReview";

export function StartupReviewRow({ item, signature, pending, busy, onToggle }: {
  item: StartupItem; signature?: StartupImpactEntry; pending: boolean; busy: boolean; onToggle: () => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const detailId = useId();
  const knownPurpose = !!item.Description && item.Description !== "Unknown Application";
  return <article className="startup-review-row">
    <div className="startup-review-row-header">
      <button type="button" className="startup-review-identity" aria-expanded={expanded} aria-controls={detailId} onClick={() => setExpanded(value => !value)}>
        <span className="startup-review-icon"><Icon icon={item.Source === "Folder" || item.Source === "File" ? "folder-close" : "application"} size={22} /></span>
        <span><strong>{item.Name}</strong><small>{signature?.signer || (item.Category && item.Category !== "Unknown" ? item.Category : "Publisher unavailable")} · {item.Source === "Registry" ? "Registry entry" : "Startup shortcut"}</small></span>
      </button>
      <Tag minimal intent={item.Recommendation === "Keep" ? "success" : item.Recommendation === "Disable" ? "warning" : "none"}>{item.Recommendation === "Keep" ? "Keep suggested" : item.Recommendation === "Disable" ? "Review suggested" : "Optional"}</Tag>
      <span className="startup-review-state">{item.IsEnabled ? "Enabled" : "Disabled"}</span>
      <Button small loading={pending} disabled={busy} intent={item.IsEnabled ? "none" : "success"} onClick={onToggle} aria-label={`${item.IsEnabled ? "Disable" : "Enable"} ${item.Name} at startup`}>{item.IsEnabled ? "Disable" : "Enable"}</Button>
      <button type="button" className="startup-review-expand" aria-label={`${expanded ? "Hide" : "Show"} details for ${item.Name}`} aria-expanded={expanded} aria-controls={detailId} onClick={() => setExpanded(value => !value)}><Icon icon={expanded ? "chevron-up" : "chevron-down"} size={16} /></button>
    </div>
    <div id={detailId} hidden={!expanded} className="startup-review-details">
      <dl>
        <div><dt>Purpose</dt><dd>{knownPurpose ? `${item.Description} (matched by entry name or command).` : "Unavailable — review the application and its launcher before changing it."}</dd></div>
        <div><dt>Publisher</dt><dd>{signature?.signer || "Unavailable"}{signature ? ` · Signature: ${signature.signatureStatus}` : ""}</dd></div>
        <div><dt>Launch command</dt><dd className="startup-review-command">{item.Command || "Unavailable"}</dd></div>
        <div><dt>Startup location</dt><dd className="startup-review-command">{item.Location}</dd></div>
        <div><dt>Impact</dt><dd>Startup delay is not measured.{item.Status === "Running" && Number.isFinite(item.RamUsageMB) ? ` Related process names currently use ${item.RamUsageMB.toFixed(0)} MiB of memory; this does not measure startup delay or memory saved.` : " No related running process was detected."}</dd></div>
      </dl>
      <p>Disabling prevents automatic launch; it does not uninstall the app. Syncing, updates or notifications may wait until you open it. Suggestions are a guide, not a measurement.</p>
    </div>
  </article>;
}
