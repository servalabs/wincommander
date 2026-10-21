import { useEffect, useState } from "react";
import { Icon, CheckboxControl, Button } from "@/components/ui/bp";
import { diskNodeColor, diskPathContains, diskPercent, formatDiskBytes, type DiskNode, type LargeDiskItem } from "@/lib/diskAnalyzer";

interface Props {
    items: DiskNode[];
    total: number;
    selected: Set<string>;
    focused: DiskNode | null;
    large?: boolean;
    disabled: boolean;
    onFocus: (node: DiskNode) => void;
    onSelect: (node: DiskNode) => void;
    onNavigate: (path: string) => void;
}

export default function DiskItemList({ items, total, selected, focused, large, disabled, onFocus, onSelect, onNavigate }: Props) {
    const [visibleCount, setVisibleCount] = useState(150);
    useEffect(() => setVisibleCount(150), [items]);
    const focusedIndex = focused ? items.findIndex(item => item.fullPath === focused.fullPath || item.isDir && diskPathContains(item.fullPath, focused.fullPath)) : -1;
    const shownCount = Math.max(visibleCount, focusedIndex + 1);
    return <div className={`da-item-list${large ? " da-item-list-large" : ""}`} aria-label={large ? "Large files and folders" : "Folder contents"}>
        {items.slice(0, shownCount).map((item, index) => {
            const percent = diskPercent(item.size, total);
            const largeItem = item as LargeDiskItem;
            const isFocused = focusedIndex === index;
            return <div key={item.fullPath} className={`da-item${isFocused ? " active" : ""}${selected.has(item.fullPath) ? " selected" : ""}`}>
                <span title={item.isDir ? "Explore this folder to select individual files. Directory deletion is unavailable." : undefined}><CheckboxControl checked={selected.has(item.fullPath)} disabled={disabled || item.isDir} ariaLabel={`Select ${item.name}${item.isDir ? ". Folder deletion unavailable" : ""}`} onChange={() => onSelect(item)} /></span>
                <button type="button" className="da-item-main" disabled={disabled} onClick={() => onFocus(item)}
                    onDoubleClick={() => item.isDir && onNavigate(item.fullPath)} title={item.fullPath} aria-pressed={isFocused}>
                    <Icon icon={item.isDir ? "folder-close" : "document"} size={24} style={{ color: diskNodeColor(item, index) }} />
                    <span className="da-item-copy"><strong>{item.name}</strong><small>{large ? item.fullPath : item.isDir ? `${item.fileCount.toLocaleString()} files · ${item.folderCount.toLocaleString()} folders` : "File"}</small></span>
                    <span className="da-item-size"><strong>{formatDiskBytes(item.size)}</strong><small>{percent.toFixed(1)}%</small>
                        <span className="da-item-meter"><span style={{ width: `${percent}%`, background: diskNodeColor(item, index) }} /></span>
                    </span>
                </button>
                {large && <span className={`da-risk-tag risk-${largeItem.risk}`}>{largeItem.risk === "high" ? "High risk" : largeItem.risk === "medium" ? "Review" : largeItem.risk || "Review"}</span>}
                {item.isDir && <Button minimal small icon="chevron-right" disabled={disabled} aria-label={`Open ${item.name}`} title={`Open ${item.name}`} onClick={() => onNavigate(item.fullPath)} />}
            </div>;
        })}
        {items.length > shownCount && <div className="da-list-more"><Button small minimal onClick={() => setVisibleCount(shownCount + 150)}>Show 150 more ({(items.length - shownCount).toLocaleString()} remaining)</Button></div>}
        {!items.length && <div className="da-chart-empty">No items match this view.</div>}
    </div>;
}
