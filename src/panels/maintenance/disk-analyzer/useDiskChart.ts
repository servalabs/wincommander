import { useEffect, useRef, useState } from "react";
import type { DiskNode } from "@/lib/diskAnalyzer";
import type { DiskChartNode } from "@/lib/diskAnalyzerLayout";
import { loadDiskChartTree } from "@/lib/diskChartTree";

export function useDiskChart(rows: DiskNode[], depth: number, getChildren: (path: string) => Promise<DiskNode[]>, disabled: boolean) {
    const loader = useRef(getChildren);
    loader.current = getChildren;
    const [state, setState] = useState<{ nodes: DiskChartNode[]; loading: boolean; limited: boolean }>({ nodes: [], loading: false, limited: false });
    useEffect(() => {
        let active = true;
        const nodes: DiskChartNode[] = rows.map(row => ({ ...row }));
        if (disabled || depth === 1 || !rows.length) {
            setState({ nodes, loading: false, limited: false });
            return;
        }
        setState({ nodes, loading: true, limited: false });
        void loadDiskChartTree(rows, depth, path => loader.current(path), () => active).then(result => {
            if (active) setState({ ...result, loading: false });
        });
        return () => { active = false; };
    }, [rows, depth, disabled]);
    return state;
}
