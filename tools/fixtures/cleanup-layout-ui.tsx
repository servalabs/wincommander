import { createRoot } from "react-dom/client";
import "../../src/index.css";
import "../../src/styles/v2-theme.css";
import "../../src/panels/cleanup/cleanupDisplay.css";
import "../../src/panels/maintenance/DiskSpaceAnalyzerDialog.css";
import CleanupRecordPreview from "../../src/components/cleanup/CleanupRecordPreview";
import CleanupTraceCard from "../../src/components/cleanup/CleanupTraceCard";
import DiskItemDetails from "../../src/panels/maintenance/disk-analyzer/DiskItemDetails";
import { ALL_CATEGORIES } from "../../src/panels/cleanup/cleanupCategories";
import { MotionPreferenceProvider } from "../../src/hooks/useMotionPreference";

const category = (id: string) => ALL_CATEGORIES.find(item => item.id === id)!;
const noop = () => {};
const dns = { entries: Array.from({ length: 12 }, (_, index) => ({
  name: `sample-${index}.example.invalid`, value: "192.0.2.1", type: "", status: "Success",
  section: 1, ttl: 245, typeCode: 0, dataLength: 4, source: index === 11 ? "Later-page source" : "",
})) };
const longName = `${"long-file-name-".repeat(12)}.txt`;
const item = { entryId: 1, name: longName, fullPath: `C:\\Synthetic\\${"nested-folder\\".repeat(14)}${longName}`,
  size: 1024, allocated: null, isDir: false, lastModified: null, fileCount: 1, folderCount: 0 };

document.body.style.cssText = "margin:0;padding:20px;min-width:0;height:auto;overflow:auto;background:var(--color-bg-card);color:var(--color-text-primary)";
createRoot(document.getElementById("fixture")!).render(<MotionPreferenceProvider>
  <h1>Cleanup layout checks</h1><p>Synthetic records. No Windows changes.</p>
  <div style={{ display: "grid", gap: 12, marginBlock: 20 }}>
    <CleanupTraceCard category={category("clipboardHistory")} compact count={-1} preview={[]} loading={false} clearing={false}
      rawData={{ historyItems: [], clipboardHistoryDisabled: true, historyReadStatus: "disabled", partial: true, status: "partial" }} />
    <CleanupTraceCard category={category("developerCaches")} compact count={0} preview={[]} loading={false} clearing={false}
      rawData={{ files: [], total: 0, coverage: "limited" }} onViewDetails={noop} />
  </div>
  <CleanupRecordPreview categoryId="dnsCache" rawData={dns} mode="advanced" />
  <h2>Selected file details</h2>
  <div className="disk-analyzer-inline" style={{ maxWidth: 900 }}><div className="da-workspace" style={{ minHeight: 0, height: "auto" }}>
    <footer className="da-selection-bar"><div className="da-selection-summary"><span><strong>2 selected</strong><small>19 KiB logical size · Space recovered may differ</small></span></div>
      <DiskItemDetails item={item} disabled={false} onOpen={noop} onNavigate={noop} onDelete={noop} />
      <div className="da-selection-actions"><button type="button">Clear</button></div>
    </footer>
  </div></div>
</MotionPreferenceProvider>);
