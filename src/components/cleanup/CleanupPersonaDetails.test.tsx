import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ALL_CATEGORIES } from "../../panels/cleanup/cleanupCategories";
import { CleanupDisplayProvider, cleanupModeForPersona, useCleanupDisplay } from "../../panels/cleanup/cleanupDisplay";
import CleanupDisplayDetails from "./CleanupDisplayDetails";
import { getSafeCleanupPreview } from "./CleanupRecordPreview";

const amcache = ALL_CATEGORIES.find(category => category.id === "amcache")!;
const raw = { total: 2, entries: [{ category: "InventoryApplication", count: 2, sample: [{ name: "Synthetic App", path: "C:\\Private\\file.txt", password: "secret-value" }] }] };

function PersonaDetail() {
  const mode = useCleanupDisplay()!;
  return <CleanupDisplayDetails category={amcache} mode={mode} count={2} items={[]} rawData={raw} clearing={false} clearLabel="Clear" onClose={() => {}} />;
}

describe("System Cleanup persona details", () => {
  test("follows the existing persona, independently of interface density", () => {
    expect(cleanupModeForPersona("casual")).toBe("simple");
    expect(cleanupModeForPersona("secure")).toBe("advanced");
    const casual = renderToStaticMarkup(<CleanupDisplayProvider persona="casual"><PersonaDetail /></CleanupDisplayProvider>);
    const secure = renderToStaticMarkup(<CleanupDisplayProvider persona="secure"><PersonaDetail /></CleanupDisplayProvider>);
    for (const html of [casual, secure]) {
      expect(html).toContain("2 records");
      expect(html).toContain("Synthetic App");
      expect(html).not.toContain("Detail level");
      expect(html).not.toContain("secret-value");
      expect(html).toContain("file.txt");
    }
    expect(casual).not.toContain("Scan coverage");
    expect(secure).toContain("Scan coverage");
    expect(secure).toContain("Record categories");
  });

  test("shows browser profiles and Wi-Fi names without passwords", () => {
    const browser = ALL_CATEGORIES.find(category => category.id === "browserFootprints")!;
    const wifi = ALL_CATEGORIES.find(category => category.id === "wlanProfiles")!;
    const browserHtml = renderToStaticMarkup(<CleanupDisplayDetails category={browser} mode="advanced" count={1} items={[]} rawData={{ browsers: [{ browser: "Google Chrome", profilePath: "C:\\Private\\Profile", totalSizeKB: 2, artifacts: [{ name: "History", sizeKB: 1 }] }] }} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(browserHtml).toContain("Google Chrome");
    expect(browserHtml).toContain("History: 1.0 KB");
    expect(browserHtml).toContain("Private\\Profile");
    const wifiHtml = renderToStaticMarkup(<CleanupDisplayDetails category={wifi} mode="simple" count={1} items={[]} rawData={{ profiles: [{ name: "PrivateNetwork", password: "secret-value" }] }} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(wifiHtml).toContain("Saved Wi-Fi profile 1");
    expect(wifiHtml).toContain("PrivateNetwork");
    expect(wifiHtml).not.toContain("secret-value");
  });

  test("shows the activity record returned by the scan", () => {
    const history = ALL_CATEGORIES.find(category => category.id === "ntUserTraces")!;
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={history} mode="advanced" count={1} items={[]} rawData={{ total: 1, sections: [{ name: "RunMRU", count: 1, entries: [{ key: "a", value: "C:\\Private\\secret.exe" }] }] }} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain("Run command 1");
    expect(html).toContain("secret.exe");
  });

  test("shows file and drive records without fabricating clipboard contents", () => {
    const clipboard = getSafeCleanupPreview("clipboardHistory", { historyItems: [{ preview: "sensitive clipboard", type: "text" }] });
    const drives = getSafeCleanupPreview("netDrives", { drives: [{ Name: "Z:", DisplayRoot: "\\\\private-server\\secret" }] });
    const recycle = getSafeCleanupPreview("recycleBin", { items: [{ name: "private file.txt", path: "C:\\private" }] });
    expect(clipboard[0].label).toBe("Clipboard item 1");
    expect(drives[0].label).toBe("Drive Z:");
    expect(recycle[0].label).toBe("Recycle Bin record 1");
    expect(JSON.stringify(clipboard)).not.toContain("sensitive clipboard");
    expect(JSON.stringify(drives)).toContain("private-server");
    expect(JSON.stringify(recycle)).toContain("private file.txt");
  });

  test("distinguishes bounded preview rows from the full count", () => {
    const samples = Array.from({ length: 30 }, (_, index) => ({ name: `Sample ${index + 1}` }));
    const source = { total: 30, entries: [{ category: "InventoryApplication", count: 30, sample: samples }] };
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={amcache} mode="advanced" count={30} items={[]} rawData={source} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain("30 records");
    expect(html).toContain("30 preview rows available here");
    expect(html).toContain("Filter preview");
    expect(html).not.toContain("Sample 30");
    expect(html).toContain("Next");
  });

  test("shows account-specific safe previews in both personas", () => {
    const groups = [{ title: "Account A", count: 1, items: [], rawData: { total: 1, entries: [{ category: "InventoryApplication", count: 1, sample: [{ name: "Example App", path: "C:\\private" }] }] } }, { title: "Account B", count: -1, items: [] }];
    for (const mode of ["simple", "advanced"] as const) {
      const html = renderToStaticMarkup(<CleanupDisplayDetails category={amcache} mode={mode} count={-1} items={[]} groupedItems={groups} clearing={false} clearLabel="Clear" onClose={() => {}} />);
      expect(html).toContain("Account A");
      expect(html).toContain("Example App");
      expect(html).toContain("This account could not be checked");
      expect(html).toContain("C:\\private");
    }
  });

  test("shows process names, file locations and ordinary commands without credential fields", () => {
    const process = getSafeCleanupPreview("processIntel", { processes: [{ name: "example.exe", pid: 42, path: "C:\\Apps\\example.exe", signed: "Valid", signer: "Example Inc", elevated: "No", password: "do-not-show" }] });
    expect(process[0].facts.join(" ")).toContain("example.exe");
    expect(process[0].facts.join(" ")).toContain("PID: 42");
    expect(process[0].facts.join(" ")).toContain("C:\\Apps\\example.exe");
    expect(JSON.stringify(process)).not.toContain("do-not-show");
    const history = getSafeCleanupPreview("psHistory", { entries: [{ command: "Get-Process" }, { command: "connect --password hunter2" }] });
    expect(history[0].facts.join(" ")).toContain("Get-Process");
    expect(JSON.stringify(history)).not.toContain("hunter2");
  });

  test("shows account activity records supplied under records", () => {
    const rows = getSafeCleanupPreview("ntUserTraces", { records: [{ name: "TypedPaths", entries: [{ value: "C:\\Reports" }] }] });
    expect(rows[0].facts.join(" ")).toContain("C:\\Reports");
  });

  test("shows the actual USB, RDP, deleted-file and volume reader fields", () => {
    // Field names mirror Get-USBDeviceHistory, Get-RDPHistory,
    // Get-RecycleBinInfo, Get-ShadowCopies and Get-NTFSJournals.
    const cases = [
      ["usbHistory", { devices: [{ friendlyName: "Example disk", deviceId: "USB\\ABC", manufacturer: "Example", className: "DiskDrive" }] }, ["Example disk", "USB\\ABC", "Example", "DiskDrive"]],
      ["rdpHistory", { entries: [{ type: "Server", host: "workstation.example", username: "example-user", key: "registry-key" }] }, ["Server", "workstation.example", "example-user"]],
      ["recycleBin", { items: [{ originalPath: "C:\\Reports\\draft.txt", deletedTime: "2026-10-20T12:00:00Z", account: "example-user", drive: "C", sizeKB: 2 }] }, ["draft.txt", "example-user", "Drive: C", "2.0 KB"]],
      ["shadowCopies", { copies: [{ id: "snapshot-123", drive: "C:", deviceObject: "\\\\?\\GLOBALROOT\\Device\\Shadow", originatingMachine: "workstation", stateStr: "Created", persistent: true, clientAccessible: false }] }, ["snapshot-123", "Drive: C:", "Shadow", "workstation", "Created", "Persistent: Yes", "Client accessible: No"]],
      ["ntfsJournals", { journals: [{ drive: "D:", journalId: "journal-123", maxSize: "0x100", present: true }] }, ["Drive: D:", "journal-123", "Maximum size: 0x100", "Present: Yes"]],
      ["prefetchFiles", { entries: [{ name: "EXAMPLE", fileName: "EXAMPLE-123.pf", sizeKB: 4, lastRun: "2026-10-20T12:00:00Z" }] }, ["EXAMPLE", "EXAMPLE-123.pf", "4.0 KB"]],
    ] as const;
    for (const [id, raw, expected] of cases) {
      const row = getSafeCleanupPreview(id, raw)[0];
      expect(Boolean(row)).toBe(true);
      const text = row.facts.join(" ");
      for (const value of expected) expect(text).toContain(value);
    }
  });

  test("accepts producer field casing and never renders credential fields", () => {
    const [dns] = getSafeCleanupPreview("dnsCache", { entries: [{ Name: "example.test", Data: "192.0.2.1", RecordType: "A", TTL: 300, Status: "Success", Password: "never-render", token: "never-render" }] });
    expect(dns.facts.join(" ")).toContain("example.test");
    expect(dns.facts.join(" ")).toContain("192.0.2.1");
    expect(dns.facts.join(" ")).toContain("Type: A");
    expect(dns.facts.join(" ")).toContain("TTL: 300");
    expect(dns.facts.join(" ")).toContain("Success");
    expect(JSON.stringify(dns)).not.toContain("never-render");
  });

  test("distinguishes same-name Wi-Fi profiles on different reported adapters", () => {
    const rows = getSafeCleanupPreview("wlanProfiles", { profiles: [
      { name: "Example Wi-Fi", interfaceId: "adapter-1", password: "never-render" },
      { name: "Example Wi-Fi", interfaceId: "adapter-2", password: "never-render" },
    ] });
    expect(rows[0].facts.join(" ")).toContain("adapter-1");
    expect(rows[1].facts.join(" ")).toContain("adapter-2");
    expect(JSON.stringify(rows)).not.toContain("never-render");
  });
});
