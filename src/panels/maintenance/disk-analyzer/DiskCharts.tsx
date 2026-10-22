import { useMemo } from "react";
import { diskNodeColor, formatDiskBytes, type DiskNode } from "@/lib/diskAnalyzer";
import { diskArcPath, diskSunburst, diskTreemap, type DiskChartNode } from "@/lib/diskAnalyzerLayout";
import { prepareDiskChart, isDiskRemainder, diskChartCoverage } from "@/lib/diskChartDisplay";
import { useDiskChartSize } from "./useDiskChartSize";

interface Props {
    nodes: DiskChartNode[];
    depth: number;
    mode: "treemap" | "sunburst";
    focused: DiskNode | null;
    onFocus: (node: DiskNode) => void;
    onNavigate: (path: string) => void;
    disabled: boolean;
    scopePath: string;
}

export default function DiskCharts({ nodes, depth, mode, focused, onFocus, onNavigate, disabled, scopePath }: Props) {
    const total = nodes.reduce((sum, node) => sum + node.size, 0);
    const chartSize = useDiskChartSize(total > 0);
    const display = useMemo(() => prepareDiskChart(nodes, depth, mode, chartSize.width, chartSize.height, scopePath), [nodes, depth, mode, chartSize.width, chartSize.height, scopePath]);
    const tiles = useMemo(() => diskTreemap(display.nodes, chartSize.width, chartSize.height, depth), [display.nodes, depth, chartSize.width, chartSize.height]);
    const arcs = useMemo(() => diskSunburst(display.nodes, depth), [display.nodes, depth]);
    const coverage = useMemo(() => {
        const paths = new Set(tiles.map(tile => tile.node.fullPath));
        const shapes = mode === "treemap" ? tiles.map(tile => ({ node: tile.node, level: tile.level,
            drawn: tile.width > 2 && tile.height > 2,
            collapsed: tile.level < depth && Boolean(tile.node.children?.length) && !tile.node.children!.some(child => paths.has(child.fullPath)) }))
            : arcs.map(arc => ({ node: arc.node, level: arc.level, drawn: arc.end - arc.start > 0.001, collapsed: false }));
        return diskChartCoverage(display.remainders, shapes, scopePath);
    }, [display.remainders, tiles, arcs, depth, mode, scopePath]);
    function interaction(node: DiskNode) {
        return {
            role: "button", tabIndex: disabled ? -1 : 0,
            "aria-label": `${node.name}, ${formatDiskBytes(node.size)}. Select for details${node.isDir ? "; Enter to open folder" : ""}.`,
            "aria-pressed": focused?.fullPath === node.fullPath,
            onClick: () => { if (!disabled) onFocus(node); },
            onDoubleClick: () => { if (!disabled && node.isDir) onNavigate(node.fullPath); },
            onKeyDown: (event: React.KeyboardEvent<SVGGElement>) => {
                if (disabled || !["Enter", " "].includes(event.key)) return;
                event.preventDefault();
                if (event.key === "Enter" && node.isDir) onNavigate(node.fullPath); else onFocus(node);
            },
        };
    }
    const graph = !total ? <div className="da-chart-empty">No non-empty items to chart. Zero-size entries remain in the folder list.</div> : mode === "treemap" ? (
        <svg ref={chartSize.ref} className="da-chart da-treemap" viewBox={`0 0 ${chartSize.width} ${chartSize.height}`} role="group" aria-label="Disk usage treemap. Areas represent logical file sizes.">
            {tiles.filter(tile => tile.width > 2 && tile.height > 2).map(tile => {
                const { node, x, y, width, height, level, colorIndex } = tile;
                const active = focused?.fullPath === node.fullPath;
                return <g key={node.fullPath} {...interaction(node)} className={`da-chart-item${active ? " active" : ""}`}>
                    <title>{isDiskRemainder(node) ? `${node.remainder.entryCount.toLocaleString()} entries grouped in Other` : node.fullPath} — {formatDiskBytes(node.size)}</title>
                    <rect x={x + 1} y={y + 1} width={Math.max(0, width - 2)} height={Math.max(0, height - 2)} rx="3"
                        fill={diskNodeColor(node, colorIndex)} fillOpacity={0.25 + level * 0.1} />
                    {width > 65 && height > 24 && <text x={x + 8} y={y + 16} fontSize="11">
                        {node.name.slice(0, Math.max(2, Math.floor((width - 18) / 6)))}
                    </text>}
                    {width > 70 && height > 43 && !(node.children?.length && level < depth) &&
                        <text x={x + 8} y={y + 34} fontSize="10" className="da-chart-size">{formatDiskBytes(node.size)}</text>}
                </g>;
            })}
        </svg>
    ) : (
        <svg ref={chartSize.ref} className="da-chart da-sunburst" viewBox="0 0 500 500" role="group" aria-label="Disk usage sunburst. Angles represent logical file sizes; rings represent folder depth.">
            {arcs.filter(arc => arc.end - arc.start > 0.001).map(arc => {
                const ring = 174 / depth;
                return <g key={arc.node.fullPath} {...interaction(arc.node)} className={`da-chart-item${focused?.fullPath === arc.node.fullPath ? " active" : ""}`}>
                    <title>{isDiskRemainder(arc.node) ? `${arc.node.remainder.entryCount.toLocaleString()} entries grouped in Other` : arc.node.fullPath} — {formatDiskBytes(arc.node.size)}</title>
                    <path d={diskArcPath(arc.start, arc.end, 62 + (arc.level - 1) * ring, 61 + arc.level * ring)}
                        fill={diskNodeColor(arc.node, arc.colorIndex)} fillOpacity={0.42 + arc.level * 0.12} />
                </g>;
            })}
            <text x="250" y="244" textAnchor="middle" className="da-sunburst-total">{formatDiskBytes(focused?.size ?? total)}</text>
            <text x="250" y="266" textAnchor="middle" className="da-sunburst-caption">{focused ? focused.name.slice(0, 15) : "Logical size"}</text>
        </svg>
    );
    return <div className="da-chart-display">{graph}<div className="da-chart-remainders" aria-label="Chart display coverage" role="status">
        {coverage.length ? <><span>Other · {coverage.length} grouped or collapsed locations:</span>{coverage.map(node => <button type="button" key={node.fullPath} disabled={disabled} onClick={() => onFocus(node)} title={`${node.remainder?.parentPath}: ${node.remainder?.reason}`}>
            {node.remainder?.entryCount.toLocaleString()} entries · {formatDiskBytes(node.size)}
        </button>)}</> : <span>All scanned items at the chosen depth have chart area. Deeper folder contents are collapsed; zero-size files have no area.</span>}
    </div></div>;
}
