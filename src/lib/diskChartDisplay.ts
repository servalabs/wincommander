import { diskPathContains, type DiskNode } from "./diskAnalyzer";
import type { DiskChartNode } from "./diskAnalyzerLayout";

function other(items: DiskChartNode[], parentPath: string, reason: string): DiskChartNode {
    const totals = items.reduce((sum, item) => ({
        size: sum.size + item.size,
        files: sum.files + (item.remainder || item.isDir ? item.fileCount : 1),
        folders: sum.folders + (item.remainder ? item.folderCount : item.isDir ? item.folderCount + 1 : 0),
    }), { size: 0, files: 0, folders: 0 });
    return { entryId: -1, name: "Other", fullPath: `other:${parentPath}`, size: totals.size,
        allocated: null, isDir: false, lastModified: "", fileCount: totals.files, folderCount: totals.folders,
        remainder: { parentPath, directItems: items.length, entryCount: totals.files + totals.folders, reason } };
}

export function prepareDiskChart(nodes: DiskChartNode[], depth: number, mode: "treemap" | "sunburst", width: number, height: number, scopePath: string, maximumNodes = 600) {
    const remainders: DiskChartNode[] = [];
    let used = 0;
    function group(items: DiskChartNode[], level: number, parentPath: string, area: number): DiskChartNode[] {
        const total = items.reduce((sum, node) => sum + Math.max(0, node.size), 0);
        const minimumShare = mode === "sunburst" ? 0.0015 : Math.min(0.03, 90 / Math.max(1, area));
        const ordered = [...items].sort((a, b) => b.size - a.size);
        const visible: DiskChartNode[] = [], omitted: DiskChartNode[] = [];
        const slots = Math.max(0, Math.min(60, maximumNodes - used - 1));
        for (const node of ordered) {
            if (visible.length >= slots || node.size <= 0 || total > 0 && node.size / total < minimumShare) omitted.push(node);
            else visible.push({ ...node, children: undefined });
        }
        used += visible.length;
        if (omitted.length) {
            const remainder = other(omitted, parentPath, "Small or zero-size items and the chart rendering limit");
            remainders.push(remainder);
            if (remainder.size > 0 && used < maximumNodes) { visible.push(remainder); used++; }
        }
        for (const node of visible) {
            if (!node.isDir || level >= depth) continue;
            const original = items.find(item => item.fullPath === node.fullPath)!;
            const childArea = total > 0 ? area * node.size / total * 0.75 : 0;
            if (original.children?.length && used < maximumNodes - 1) node.children = group(original.children, level + 1, node.fullPath, childArea);
            else if (node.fileCount || node.folderCount || node.size) {
                const remainder: DiskChartNode = { ...node, entryId: -1, name: "Other", isDir: false, children: undefined,
                    fullPath: `other:${node.fullPath}`, remainder: { parentPath: node.fullPath, directItems: original.children?.length ?? 0,
                        entryCount: node.fileCount + node.folderCount, reason: original.children ? "Chart rendering limit; folder totals remain represented" : "Folder detail is collapsed or was not loaded; folder totals remain represented" } };
                remainders.push(remainder);
                if (used < maximumNodes) { node.children = [remainder]; used++; }
            }
        }
        return visible;
    }
    return { nodes: group(nodes, 1, scopePath, width * height), remainders, renderedNodes: used };
}

export function isDiskRemainder(node: DiskNode): node is DiskChartNode & { remainder: NonNullable<DiskChartNode["remainder"]> } {
    return Boolean((node as DiskChartNode).remainder);
}

export interface DiskCoverageShape { node: DiskChartNode; level: number; drawn: boolean; collapsed: boolean }

export function diskChartCoverage(remainders: DiskChartNode[], shapes: DiskCoverageShape[], scopePath: string) {
    const hidden = shapes.filter(shape => !shape.drawn).filter(shape => !shapes.some(parent => !parent.drawn && parent.node.isDir && parent.node !== shape.node && diskPathContains(parent.node.fullPath, shape.node.remainder?.parentPath ?? shape.node.fullPath)));
    const collapsed = shapes.filter(shape => shape.drawn && shape.collapsed);
    const blocked = [...hidden, ...collapsed].map(shape => shape.node.fullPath);
    const groups = remainders.filter(group => !blocked.some(path => path === group.fullPath || diskPathContains(path, group.remainder!.parentPath)));
    const byParent = new Map<string, DiskChartNode[]>();
    for (const shape of hidden) {
        const rawParent = shape.node.fullPath.replace(/[\\/][^\\/]+$/, "");
        const parent = shape.node.remainder?.parentPath ?? (/^[a-z]:$/i.test(rawParent) ? `${rawParent}\\` : rawParent || scopePath);
        byParent.set(parent, [...byParent.get(parent) ?? [], shape.node]);
    }
    for (const [path, items] of byParent) groups.push(other(items, path, "Items are too small to draw at this window size; their bytes remain in the parent area"));
    for (const { node } of collapsed) groups.push({ ...node, entryId: -1, name: "Other", isDir: false, children: undefined,
        fullPath: `collapsed:${node.fullPath}`, remainder: { parentPath: node.fullPath, directItems: node.children?.length ?? 0,
            entryCount: node.fileCount + node.folderCount, reason: "This folder tile is too small to draw descendant detail; its full logical size remains represented" } });
    const merged = new Map<string, DiskChartNode>();
    for (const group of groups) {
        const path = group.remainder!.parentPath;
        const previous = merged.get(path);
        if (!previous) merged.set(path, { ...group, fullPath: `other:${path}`, remainder: { ...group.remainder! } });
        else {
            previous.size += group.size; previous.fileCount += group.fileCount; previous.folderCount += group.folderCount;
            previous.remainder!.entryCount += group.remainder!.entryCount;
            previous.remainder!.directItems += group.remainder!.directItems;
            if (!previous.remainder!.reason.includes(group.remainder!.reason)) previous.remainder!.reason += `; ${group.remainder!.reason}`;
        }
    }
    return [...merged.values()];
}
