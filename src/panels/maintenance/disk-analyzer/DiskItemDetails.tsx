import { isDiskRemainder } from "@/lib/diskChartDisplay";
import { Button, Icon } from "@/components/ui/bp";
import { formatDiskBytes, formatDiskModified, type DiskNode, type LargeDiskItem } from "@/lib/diskAnalyzer";
import { toExplorerDisplayPath } from "../maintenancePathDisplay";

interface Props {
    item: DiskNode | null;
    disabled: boolean;
    onOpen: (item: DiskNode) => void;
    onNavigate: (path: string) => void;
    onDelete: (item: DiskNode) => void;
}

export default function DiskItemDetails({ item, disabled, onOpen, onNavigate, onDelete }: Props) {
    if (!item) return <div className="da-item-details da-details-empty"><Icon icon="info-sign" size={16} /><span>Select an item in the list or chart to inspect it. Double-click a folder to explore it.</span></div>;
    const displayPath = (path: string) => toExplorerDisplayPath(path) ?? "Location unavailable";
    if (isDiskRemainder(item)) return <section className="da-item-details" aria-label="Selected item details"><div className="da-details-copy"><strong>Other · {item.remainder.entryCount.toLocaleString()} entries</strong>
        <span title={displayPath(item.remainder.parentPath)}>{displayPath(item.remainder.parentPath)}</span><small>{formatDiskBytes(item.size)} logical size · {item.fileCount.toLocaleString()} files · {item.folderCount.toLocaleString()} folders</small><small>{item.remainder.reason}. Open the containing folder to inspect individual files.</small></div>
        <div className="da-details-actions"><Button minimal icon="folder-open" title="Browse containing folder" aria-label="Browse containing folder" disabled={disabled} onClick={() => onNavigate(item.remainder.parentPath)} /></div></section>;
    const hint = (item as LargeDiskItem).cleanupHint;
    return <section className="da-item-details" aria-label="Selected item details">
        <div className="da-details-copy"><strong>{item.name}</strong><span title={displayPath(item.fullPath)}>{displayPath(item.fullPath)}</span>
            <small>{formatDiskBytes(item.size)} logical size{item.isDir ? ` · ${item.fileCount.toLocaleString()} files` : ""}{item.lastModified ? ` · Modified ${formatDiskModified(item.lastModified)}` : ""}</small>
            {hint && <small>{hint}</small>}
            {item.isDir && <small>Open this folder to review and select individual files. Folder deletion is disabled for safety.</small>}
        </div>
        <div className="da-details-actions">
            <Button minimal icon="folder-open" title="Open in Explorer" aria-label={`Open ${item.name} in Explorer`} disabled={disabled} onClick={() => onOpen(item)} />
            {item.isDir && <Button minimal icon="zoom-in" title="Explore folder" aria-label={`Explore ${item.name}`} disabled={disabled} onClick={() => onNavigate(item.fullPath)} />}
            <Button minimal icon="trash" intent="danger" title="Review deletion" aria-label={`Review deletion of ${item.name}`} disabled={disabled || item.isDir} onClick={() => onDelete(item)} />
        </div>
    </section>;
}
