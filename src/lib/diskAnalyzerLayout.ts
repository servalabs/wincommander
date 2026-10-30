import type { DiskNode } from "./diskAnalyzer";

export interface DiskRemainder {
    parentPath: string;
    directItems: number;
    entryCount: number;
    reason: string;
}
export interface DiskChartNode extends DiskNode { children?: DiskChartNode[]; remainder?: DiskRemainder }
export interface DiskTile { x: number; y: number; width: number; height: number; node: DiskChartNode; level: number; colorIndex: number }
export interface DiskArc { start: number; end: number; level: number; node: DiskChartNode; colorIndex: number }

export function diskTreemap(nodes: DiskChartNode[], width: number, height: number, depth: number): DiskTile[] {
    const tiles: DiskTile[] = [];
    function layout(items: DiskChartNode[], x: number, y: number, w: number, h: number, level: number, parentColor?: number) {
        const valid = items.filter(item => item.size > 0).sort((a, b) => b.size - a.size);
        const total = valid.reduce((sum, item) => sum + item.size, 0);
        if (!total || w <= 0 || h <= 0) return;
        const areas = valid.map((node, index) => ({ node, area: node.size / total * w * h, color: parentColor ?? index }));
        let offsetX = x, offsetY = y, remainingW = w, remainingH = h;
        let cursor = 0;
        function ratio(row: typeof areas, side: number) {
            const sum = row.reduce((s, item) => s + item.area, 0);
            return Math.max(...row.map(item => Math.max(sum * sum / (side * side * item.area), side * side * item.area / (sum * sum))));
        }
        while (cursor < areas.length) {
            const vertical = remainingW >= remainingH;
            const side = Math.max(1, vertical ? remainingH : remainingW);
            const row = [areas[cursor++]];
            while (cursor < areas.length && ratio([...row, areas[cursor]], side) <= ratio(row, side)) row.push(areas[cursor++]);
            const sum = row.reduce((s, item) => s + item.area, 0);
            const band = sum / side;
            let offset = vertical ? offsetY : offsetX;
            for (const item of row) {
                const length = item.area / band;
                const tile = { x: vertical ? offsetX : offset, y: vertical ? offset : offsetY,
                    width: vertical ? band : length, height: vertical ? length : band,
                    node: item.node, level, colorIndex: item.color };
                tiles.push(tile);
                if (level < depth && item.node.children?.length && tile.width > 40 && tile.height > 44) {
                    // Keep unexpanded/direct bytes as unlabelled parent area, rather than inflating children.
                    const childTotal = item.node.children.reduce((s, child) => s + child.size, 0);
                    const fraction = Math.min(1, childTotal / item.node.size);
                    layout(item.node.children, tile.x + 2, tile.y + 22, Math.max(0, tile.width - 4),
                        Math.max(0, tile.height - 24) * fraction, level + 1, item.color);
                }
                offset += length;
            }
            if (vertical) { offsetX += band; remainingW -= band; }
            else { offsetY += band; remainingH -= band; }
        }
    }
    layout(nodes, 0, 0, width, height, 1);
    return tiles;
}

export function diskSunburst(nodes: DiskChartNode[], depth: number): DiskArc[] {
    const arcs: DiskArc[] = [];
    const total = nodes.reduce((sum, node) => sum + Math.max(0, node.size), 0);
    function layout(items: DiskChartNode[], start: number, span: number, parentSize: number, level: number, parentColor?: number) {
        let cursor = start;
        items.filter(node => node.size > 0).sort((a, b) => b.size - a.size).forEach((node, index) => {
            const angle = parentSize > 0 ? Math.min(span, node.size / parentSize * span) : 0;
            const colorIndex = parentColor ?? index;
            arcs.push({ start: cursor, end: cursor + angle, node, level, colorIndex });
            if (level < depth && node.children?.length) layout(node.children, cursor, angle, node.size, level + 1, colorIndex);
            cursor += angle;
        });
    }
    if (total) layout(nodes, 0, Math.PI * 2, total, 1);
    return arcs;
}

export function diskArcPath(start: number, end: number, inner: number, outer: number): string {
    const span = Math.min(end - start, Math.PI * 2 - 0.00001);
    const stop = start + span;
    const point = (radius: number, angle: number) => `${250 + radius * Math.sin(angle)},${250 - radius * Math.cos(angle)}`;
    const large = span > Math.PI ? 1 : 0;
    return `M${point(outer, start)} A${outer},${outer} 0 ${large} 1 ${point(outer, stop)} L${point(inner, stop)} A${inner},${inner} 0 ${large} 0 ${point(inner, start)} Z`;
}
