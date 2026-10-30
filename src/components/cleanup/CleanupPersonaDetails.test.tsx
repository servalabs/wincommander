import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ALL_CATEGORIES } from "../../panels/cleanup/cleanupCategories";
import { CleanupDisplayProvider, cleanupModeForPersona, useCleanupDisplay } from "../../panels/cleanup/cleanupDisplay";
import CleanupDisplayDetails from "./CleanupDisplayDetails";
import { getSafeCleanupPreview, mergeExpandedCleanupPage, readExpandedCleanupSummary, shouldLoadCurrentUserDetails } from "./CleanupRecordPreview";

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
      expect(html).toContain("secret-value");
      expect(html).toContain("file.txt");
    }
    expect(casual).not.toContain("Scan coverage");
    expect(secure).toContain("Scan coverage");
    expect(secure).toContain("Record categories");
  });

  test("does not request current-user secrets while inspecting another account", () => {
    expect(shouldLoadCurrentUserDetails("wlanProfiles")).toBe(true);
    expect(shouldLoadCurrentUserDetails("clipboardHistory")).toBe(true);
    expect(shouldLoadCurrentUserDetails("wlanProfiles", "Another account")).toBe(false);
    expect(shouldLoadCurrentUserDetails("clipboardHistory", "Another account")).toBe(false);
  });

  test("shows available browser and Wi-Fi details directly", () => {
    const browser = ALL_CATEGORIES.find(category => category.id === "browserFootprints")!;
    const wifi = ALL_CATEGORIES.find(category => category.id === "wlanProfiles")!;
    const browserHtml = renderToStaticMarkup(<CleanupDisplayDetails category={browser} mode="advanced" count={1} items={[]} rawData={{ browsers: [{ browser: "Google Chrome", profilePath: "C:\\Private\\Profile", totalSizeKB: 2, artifacts: [{ name: "History", sizeKB: 1, profilePath: "C:\\Private\\Profile\\Profile 1" }] }] }} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(browserHtml).toContain("Google Chrome");
    expect(browserHtml).toContain(">Artifact</th>");
    expect(browserHtml).toContain(">History</td>");
    expect(browserHtml).toContain("C:\\Private\\Profile\\Profile 1");
    expect(browserHtml).toContain(">1.0 KB</td>");
    expect(browserHtml).toContain("Private\\Profile");
    const wifiHtml = renderToStaticMarkup(<CleanupDisplayDetails category={wifi} mode="simple" count={1} items={[]} rawData={{ profiles: [{ name: "PrivateNetwork", password: "secret-value" }] }} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(wifiHtml).toContain("Found details");
    expect(wifiHtml).toContain("PrivateNetwork");
    expect(wifiHtml).not.toContain("Show passwords");
    expect(wifiHtml).toContain("secret-value");
  });

  test("shows browser artifact profile paths from each installed profile", () => {
    const rows = getSafeCleanupPreview("browserFootprints", { browsers: [{ browser: "Google Chrome", artifacts: [
      { name: "History", profilePath: "C:\\Browser\\Profile 1" },
      { name: "Cookies", profilePath: "C:\\Browser\\Profile 2" },
    ] }] });
    expect(rows[0].facts.join(" ")).toContain("Profile 1");
    expect(rows[0].facts.join(" ")).toContain("Profile 2");
  });

  test("shows the activity record returned by the scan", () => {
    const history = ALL_CATEGORIES.find(category => category.id === "ntUserTraces")!;
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={history} mode="advanced" count={1} items={[]} rawData={{ total: 1, sections: [{ name: "RunMRU", count: 1, entries: [{ key: "a", value: "C:\\Private\\secret.exe" }] }] }} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain("Run command");
    expect(html).toContain("secret.exe");
  });

  test("shows supplied clipboard, file and drive records", () => {
    const clipboard = getSafeCleanupPreview("clipboardHistory", { historyItems: [{ preview: "sensitive clipboard", type: "text" }] });
    const drives = getSafeCleanupPreview("netDrives", { drives: [{ Name: "Z:", DisplayRoot: "\\\\private-server\\secret" }] });
    const recycle = getSafeCleanupPreview("recycleBin", { items: [{ name: "private file.txt", path: "C:\\private" }] });
    expect(clipboard[0].label).toBe("Clipboard item 1");
    expect(drives[0].label).toBe("Drive Z:");
    expect(recycle[0].label).toBe("Recycle Bin record 1");
    expect(JSON.stringify(clipboard)).toContain("sensitive clipboard");
    expect(JSON.stringify(drives)).toContain("private-server");
    expect(JSON.stringify(recycle)).toContain("private file.txt");
  });

  test("shows Wi-Fi passwords returned in another account's records without looking up the current account", () => {
    const wifi = ALL_CATEGORIES.find(category => category.id === "wlanProfiles")!;
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={wifi} mode="advanced" count={1} items={[]}
      groupedItems={[{ title: "Other user", count: 1, items: [], rawData: { records: [{ name: "Sample network", interfaceId: "adapter-1", password: "example-password" }] } }]}
      clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain("example-password");
    expect(html).not.toContain("Unavailable for this account");
    expect(shouldLoadCurrentUserDetails("wlanProfiles", "Other user")).toBe(false);
  });

  test("warns when another account's results are only a partial preview", () => {
    const history = ALL_CATEGORIES.find(category => category.id === "psHistory")!;
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={history} mode="simple" count={4} items={[]}
      groupedItems={[{ title: "Other user", username: "other-folder", count: 4, items: [], rawData: { records: [{ command: "first" }, { command: "second" }], truncated: true } }]}
      clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain("Only part of this scan is shown");
    expect(html).toContain("Load more account details");
    expect(html).toContain("first");
  });

  test("accepts expanded records only from the requested account and card", () => {
    const category = { id: "psHistory", count: 2, items: ["one", "two"], records: [{ command: "one" }, { command: "two" }], sampleCount: 2, previewLimit: 50000, truncated: false, snapshotHash: `sha256:${"a".repeat(64)}`, nextOffset: null, totalRows: 2 };
    const data = { users: [{ username: "folder-name", categories: [category] }] };
    expect(readExpandedCleanupSummary(data, "folder-name", "psHistory")?.count).toBe(2);
    expect(readExpandedCleanupSummary(data, "another-user", "psHistory")).toBeNull();
    expect(readExpandedCleanupSummary(data, "folder-name", "recentFiles")).toBeNull();
    expect(readExpandedCleanupSummary({ users: [...data.users, ...data.users] }, "folder-name", "psHistory")).toBeNull();
    expect(readExpandedCleanupSummary({ users: [{ username: "folder-name", categories: [{ ...category, error: true }] }] }, "folder-name", "psHistory")).toBeNull();
    expect(readExpandedCleanupSummary(data, "folder-name", "psHistory", 0, `sha256:${"b".repeat(64)}`)).toBeNull();
    expect(readExpandedCleanupSummary({ users: [{ username: "folder-name", categories: [{ ...category, nextOffset: 1 }] }] }, "folder-name", "psHistory")).toBeNull();
    const later = { ...category, count: 3, records: [{ command: "three" }], sampleCount: 1, totalRows: 3 };
    expect(readExpandedCleanupSummary({ users: [{ username: "folder-name", categories: [later] }] }, "folder-name", "psHistory", 2, category.snapshotHash)?.count).toBe(3);
    expect(readExpandedCleanupSummary({ users: [{ username: "folder-name", categories: [later] }] }, "folder-name", "psHistory", 1, category.snapshotHash)).toBeNull();
  });

  test("merged account pages clear the partial warning only after the final complete page", () => {
    const first = mergeExpandedCleanupPage(null, { records: [{ command: "one" }, { command: "two" }], truncated: true }, 0)!;
    expect(first.truncated).toBe(true);
    const last = mergeExpandedCleanupPage(first, { records: [{ command: "three" }], truncated: false }, 2)!;
    expect(last.truncated).toBe(false);
    expect(last.records).toEqual([{ command: "one" }, { command: "two" }, { command: "three" }]);
    expect(last.sampleCount).toBe(3);
    expect(mergeExpandedCleanupPage(first, { records: [{ command: "three" }], truncated: false }, 1)).toBeNull();
    const limited = mergeExpandedCleanupPage(first, { records: [{ command: "three" }], truncated: true }, 2)!;
    expect(limited.truncated).toBe(true);
  });

  test("distinguishes bounded preview rows from the full count", () => {
    const samples = Array.from({ length: 30 }, (_, index) => ({ name: `Sample ${index + 1}` }));
    const source = { total: 30, entries: [{ category: "InventoryApplication", count: 30, sample: samples }] };
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={amcache} mode="advanced" count={30} items={[]} rawData={source} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain("30 records");
    expect(html).toContain("30 items available");
    expect(html).toContain("Filter details");
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

  test("shows all captured process fields and command text", () => {
    const process = getSafeCleanupPreview("processIntel", { processes: [{ name: "example.exe", pid: 42, path: "C:\\Apps\\example.exe", signed: "Valid", signer: "Example Inc", elevated: "No", password: "do-not-show" }] });
    expect(process[0].facts.join(" ")).toContain("example.exe");
    expect(process[0].facts.join(" ")).toContain("PID: 42");
    expect(process[0].facts.join(" ")).toContain("C:\\Apps\\example.exe");
    expect(JSON.stringify(process)).toContain("do-not-show");
    const history = getSafeCleanupPreview("psHistory", { entries: [{ command: "Get-Process" }, { command: "connect --password hunter2" }] });
    expect(history[0].facts.join(" ")).toContain("Get-Process");
    expect(JSON.stringify(history)).toContain("hunter2");
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

  test("accepts producer field casing and displays returned credential fields", () => {
    const [dns] = getSafeCleanupPreview("dnsCache", { entries: [{ Name: "example.test", Data: "192.0.2.1", RecordType: "A", TTL: 300, Status: "Success", Password: "never-render", token: "never-render" }] });
    expect(dns.facts.join(" ")).toContain("example.test");
    expect(dns.facts.join(" ")).toContain("192.0.2.1");
    expect(dns.facts.join(" ")).toContain("Type: A");
    expect(dns.facts.join(" ")).toContain("TTL: 300");
    expect(dns.facts.join(" ")).toContain("Success");
    expect(JSON.stringify(dns)).toContain("never-render");
  });

  test("shows full clipboard content including whitespace and empty values", () => {
    const value = `${"a".repeat(120)}\n  line two\tend`;
    const [clipboard] = getSafeCleanupPreview("clipboardHistory", { historyItems: [{ preview: value, type: "text", charCount: value.length, extra: "" }] });
    expect(clipboard.facts.join(" ")).toContain(value);
    expect(clipboard.facts.join(" ")).toContain("Extra: (empty)");
  });

  test("preserves unrecognised and simultaneously supplied fields", () => {
    const [row] = getSafeCleanupPreview("dnsCache", { entries: [{ name: "name", displayName: "display", metadata: { source: "adapter" }, tags: ["first", "second"], token: "sample-token" }] });
    for (const value of ["name", "display", "adapter", "first", "second", "sample-token"]) expect(row.facts.join(" ")).toContain(value);
  });

  test("renders each DNS field in its own column with a compact number", () => {
    const dns = ALL_CATEGORIES.find(category => category.id === "dnsCache")!;
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={dns} mode="advanced" count={1} items={[]} rawData={{ entries: [{ Name: "example.test", Data: "192.0.2.1", TTL: 300, Status: "Success" }] }} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    for (const label of ["Name", "Value", "Status", "TTL"]) expect(html).toContain(`>${label}</th>`);
    for (const value of ["example.test", "192.0.2.1", "Success", "300"]) expect(html).toContain(`>${value}</td>`);
    expect(html).not.toContain("Name: example.test ·");
    expect(html).not.toContain("Record 1</td>");
  });

  test("retains distinct returned timestamps and DNS fields", () => {
    const [prefetch] = getSafeCleanupPreview("prefetchFiles", { entries: [{ name: "App", lastRun: "2026-10-20T12:00:00Z", created: "2026-10-19T12:00:00Z" }] });
    expect(prefetch.facts.join(" ")).toContain("Last run:");
    expect(prefetch.facts.join(" ")).toContain("Created:");
    const [dns] = getSafeCleanupPreview("dnsCache", { entries: [{ name: "example.test", section: "Answer", dataLength: 4, recordTypeRaw: 1 }] });
    for (const value of ["Section: Answer", "Data length: 4", "Type code: 1"]) expect(dns.facts.join(" ")).toContain(value);
  });

  test("shows supplied registry keys and Wi-Fi keys", () => {
    const history = getSafeCleanupPreview("rdpHistory", { entries: [{ host: "example", key: "HKCU\\Software\\Example" }] });
    expect(history[0].facts.join(" ")).toContain("HKCU\\Software\\Example");
    const wifi = getSafeCleanupPreview("wlanProfiles", { profiles: [{ name: "Example", key: "secret-value" }] });
    expect(JSON.stringify(wifi)).toContain("secret-value");
  });

  test("distinguishes same-name Wi-Fi profiles on different reported adapters", () => {
    const rows = getSafeCleanupPreview("wlanProfiles", { profiles: [
      { name: "Example Wi-Fi", interfaceId: "adapter-1", password: "never-render" },
      { name: "Example Wi-Fi", interfaceId: "adapter-2", password: "never-render" },
    ] });
    expect(rows[0].facts.join(" ")).toContain("adapter-1");
    expect(rows[1].facts.join(" ")).toContain("adapter-2");
    expect(JSON.stringify(rows)).toContain("never-render");
  });
});
