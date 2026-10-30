import type { DiskNode } from "./diskAnalyzer";
import type { DiskChartNode } from "./diskAnalyzerLayout";

export async function loadDiskChartTree(
  rows: DiskNode[],
  depth: number,
  getChildren: (path: string) => Promise<DiskNode[]>,
  isCurrent: () => boolean = () => true,
) {
  const nodes: DiskChartNode[] = rows.map(row => ({ ...row }));
  let requests = 0;
  let limited = false;
  async function expand(items: DiskChartNode[], level: number) {
    if (level >= depth) return;
    for (const node of [...items].sort((left, right) => right.size - left.size)) {
      if (!isCurrent()) return;
      if (!node.isDir || node.size <= 0) continue;
      if (requests >= 60) { limited = true; continue; }
      requests++;
      try {
        node.children = (await getChildren(node.fullPath)).map(child => ({ ...child }));
        await expand(node.children, level + 1);
      } catch { limited = true; }
    }
  }
  await expand(nodes, 1);
  return { nodes, limited };
}
