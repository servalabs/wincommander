import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { DuplicateScanStatus, duplicateDiscoveryGuidance } from "./DuplicateScanStatus";
import type { DuplicateProgress, DuplicateScan } from "../../hooks/useBackend";

const common = { scopeRoots: ["C:\\"], excludedVolumes: [], errors: [], skippedEntries: 0, skippedLinks: 0, minimumSizeBytes: 0, skippedSmallFiles: 0, filterExcludedFiles: 2, filterExcludedDirectories: 1 };
test("reports filtered files and uncounted folder contents in live and final status", () => {
  const progress = { ...common, running: true, phase: "enumerating", scannedFiles: 3, hashedFiles: 0, candidateFiles: 0, bytesHashed: 0, currentPath: "" } as DuplicateProgress;
  const scan = { ...common, cancelled: false, complete: false } as DuplicateScan;
  for (const html of [renderToStaticMarkup(<DuplicateScanStatus progress={progress} scanning local />), renderToStaticMarkup(<DuplicateScanStatus scan={scan} scanning={false} local />)]) {
    expect(html).toContain("2 files excluded by local scan filters");
    expect(html).toContain("1 dot-named folder excluded (contents not counted)");
  }
  expect(renderToStaticMarkup(<DuplicateScanStatus scan={scan} scanning={false} local />)).toContain("Scan finished · exclusions shown");
});

test("guidance preserves minimum-size and safety limits for chosen folders", () => {
  const html = renderToStaticMarkup(<span>{duplicateDiscoveryGuidance}</span>);
  expect(html).toContain("Choose folders to scan without these local-drive filters");
  expect(html).toContain("the minimum size and other safety exclusions still apply");
});
