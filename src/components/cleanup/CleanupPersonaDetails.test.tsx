import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ALL_CATEGORIES } from "../../panels/cleanup/cleanupCategories";
import { CleanupDisplayProvider, cleanupModeForPersona, useCleanupDisplay } from "../../panels/cleanup/cleanupDisplay";
import { readCleanupUserSummary } from "../../panels/cleanup/cleanupScanResult";
import CleanupDisplayDetails from "./CleanupDisplayDetails";
import { assembleCleanupRecordChunks, ClipboardImage, ClipboardItemDetails, ClipboardStorageDetails, clipboardDetailRecord, readClipboardImageChunk, readClipboardDetailChunk, parseClipboardDetailBytes, clipboardEmptyMessage, clipboardRefreshNotice, preserveClipboardScanAfterFailedRefresh, decodeCleanupRecordChunk, getSafeCleanupPreview, mergeExpandedCleanupPage, readExpandedCleanupSummary, shouldLoadCurrentUserDetails, wifiPasswordForProfile } from "./CleanupRecordPreview";

const amcache = ALL_CATEGORIES.find(category => category.id === "amcache")!;
const raw = { total: 2, entries: [{ category: "InventoryApplication", count: 2, sample: [{ name: "Synthetic App", path: "C:\\Private\\file.txt", password: "secret-value" }] }] };

function PersonaDetail() {
  const mode = useCleanupDisplay()!;
  return <CleanupDisplayDetails category={amcache} mode={mode} count={2} items={[]} rawData={raw} clearing={false} clearLabel="Clear" onClose={() => {}} />;
}

describe("System Cleanup persona details", () => {
  test("loads bounded clipboard content without mixing items or snapshots", async () => {
    const html = renderToStaticMarkup(<ClipboardItemDetails itemId="item-1" load={async () => ({ success: false })} />);
    expect(html).toContain("Load details");
    const bytes = new TextEncoder().encode(JSON.stringify({ id: "item-1", text: "多言語 \n content", storageItems: [{ name: "file", path: "C:\\Example", type: "File" }] }));
    const digest = await crypto.subtle.digest("SHA-256", bytes);
    const hash = Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2, "0")).join("");
    const encoded = btoa(Array.from(bytes, byte => String.fromCharCode(byte)).join(""));
    const value = { status: "complete", detailBase64: encoded, offset: 0, totalBytes: bytes.length, nextOffset: null, snapshotHash: hash };
    const chunk = readClipboardDetailChunk(value, 0)!;
    expect(await parseClipboardDetailBytes(chunk.bytes, hash, "item-1")).toEqual({ id: "item-1", text: "多言語 \n content", storageItems: [{ name: "file", path: "C:\\Example", type: "File" }] });
    expect(await parseClipboardDetailBytes(chunk.bytes, "0".repeat(64), "item-1")).toBeNull();
    expect(await parseClipboardDetailBytes(chunk.bytes, hash, "item-2")).toBeNull();
    expect(readClipboardDetailChunk(value, 0, "0".repeat(64))).toBeNull();
    expect(readClipboardDetailChunk({ ...value, nextOffset: bytes.length }, 0)).toBeNull();
    expect(readClipboardDetailChunk({ ...value, totalBytes: 64 * 1024 * 1024 + 1 }, 0)).toBeNull();
    expect(readClipboardDetailChunk({ ...value, totalBytes: bytes.length + 1, nextOffset: bytes.length }, 0)).toBeNull();
  });
  test("shows clipboard files and folders in separate columns without losing unknown fields", () => {
    const items = [{ name: "Example.txt", path: "C:\\Example\\Example.txt", type: "File" }, { name: "Folder", path: "C:\\Example\\Folder", type: "Folder" }];
    const html = renderToStaticMarkup(<ClipboardStorageDetails items={items} />);
    expect(html).toContain('<th scope="col">Name</th>');
    expect(html).toContain('<th scope="col">Location</th>');
    expect(html).toContain('<th scope="col">Type</th>');
    expect(html).toContain("C:\\Example\\Example.txt");
    expect(html).toContain("C:\\Example\\Folder");
    expect(clipboardDetailRecord({ storageItems: items }, true).storageItems).toBeUndefined();
    const extra = [{ ...items[0], size: 42 }];
    expect(clipboardDetailRecord({ storageItems: extra }, true).storageItems).toEqual(extra);
    expect(getSafeCleanupPreview("clipboardHistory", { historyItems: [{ storageItems: items }] })[0].facts.join(" ")).toContain("Example.txt");
  });
  test("does not repeat clipboard text or claim an image needs another viewer", () => {
    const text = clipboardDetailRecord({ text: "example text", preview: "example text", formats: ["Text"] });
    expect(text.text).toBe("example text");
    expect(text.preview).toBeUndefined();
    const image = clipboardDetailRecord({ formats: ["Bitmap"], preview: "[Content requires another viewer]" });
    expect(image.preview).toBeUndefined();
    expect(image.formats).toEqual(["Bitmap"]);
    const unavailable = clipboardDetailRecord({ formats: ["Other"], preview: "[Content requires another viewer]" });
    expect(unavailable.preview).toBe("[Content requires another viewer]");
  });
  test("shows a load control for clipboard images and accepts only bounded raster data", () => {
    const html = renderToStaticMarkup(<ClipboardImage itemId="synthetic-id" load={async () => ({ success: false })} />);
    expect(html).toContain("Load image");
    const value = { status: "complete", imageMime: "image/png", imageBase64: "AQID", offset: 0, totalBytes: 3, nextOffset: null, snapshotHash: "a".repeat(64) };
    expect([...readClipboardImageChunk(value, 0)!.bytes]).toEqual([1, 2, 3]);
    expect(readClipboardImageChunk({ ...value, imageMime: "image/svg+xml" }, 0)).toBeNull();
    expect(readClipboardImageChunk({ ...value, imageBase64: "bad!" }, 0)).toBeNull();
    expect(readClipboardImageChunk({ ...value, status: "too-large" }, 0)).toBeNull();
    expect(readClipboardImageChunk({ ...value, nextOffset: 3 }, 0)).toBeNull();
    expect(readClipboardImageChunk(value, 0, "b".repeat(64))).toBeNull();
    expect(readClipboardImageChunk({ ...value, totalBytes: 64 * 1024 * 1024 + 1 }, 0)).toBeNull();
  });
  test("retains numeric and boolean observations in record arrays", () => {
    const rows = getSafeCleanupPreview("execCache", { records: [0, false, 42, true] });
    expect(rows.map(row => row.facts)).toEqual([["Value: 0"], ["Value: No"], ["Value: 42"], ["Value: Yes"]]);
    const grouped = getSafeCleanupPreview("ntUserTraces", { sections: [{ name: "Other", entries: [0, false] }] });
    expect(grouped.map(row => row.facts)).toEqual([["Section source: Other", "Value: 0"], ["Section source: Other", "Value: No"]]);
  });
  test("keeps colons in plain returned records as values, not column headings", () => {
    const category = ALL_CATEGORIES.find(item => item.id === "execCache")!;
    const data = { records: ["Application: sample.exe", "Path: C:\\Example"], total: 2 };
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={category} mode="advanced" count={2} items={[]} rawData={data} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain('<th scope="col">Value</th>');
    expect(html).toContain("Application: sample.exe");
    expect(html).toContain("Path: C:\\Example");
    expect(html).not.toContain('<th scope="col">Application</th>');
    expect(html).not.toContain('<th scope="col">Path</th>');
  });
  test("retains the original timestamp when showing local time", () => {
    const original = "2026-10-20T12:00:00.123+05:30";
    const [row] = getSafeCleanupPreview("prefetchFiles", { entries: [{ name: "App", lastRun: original }] });
    expect(row.facts.find(fact => fact.startsWith("Last run:"))).toContain(original);
  });

  test("preserves the size reported by a scan when formatting units", () => {
    const rows = getSafeCleanupPreview("webCache", { files: [
      { name: "A", sizeKB: 100.6 },
      { name: "B", sizeKB: 1537.2, sizeBytes: 1574092 },
    ] });
    expect(rows[0].facts).toContain("Size: 100.6 KB");
    expect(rows[1].facts).toContain("Size: 1.5 MB (1537.2 KB)");
    expect(rows[1].facts).toContain("Size Bytes: 1574092");
  });
  test("explains when clipboard details come from a newer or incomplete read", () => {
    const scan = { historyReadStatus: "complete", historyItems: [{}, {}] };
    expect(clipboardRefreshNotice(scan, { historyReadStatus: "complete", historyItems: [] })).toContain("changed since the scan");
    expect(clipboardRefreshNotice(scan, { historyReadStatus: "complete", historyItems: [{}, {}] })).toContain("newer check");
    expect(clipboardRefreshNotice(scan, { historyReadStatus: "unavailable", historyItems: [] })).toContain("earlier scan");
    expect(clipboardEmptyMessage([], "Clipboard history is disabled", "", true)).toContain("contents are unknown");
    expect(clipboardEmptyMessage([], null, "", false)).toContain("No clipboard items");
    expect(clipboardEmptyMessage([], "Clipboard history is disabled", "search", true)).toContain("No returned items match");
  });
  test("keeps earlier clipboard observations when an empty refresh fails", () => {
    const scan = { historyReadStatus: "complete", historyItems: [{ preview: "previously observed" }] };
    const failed = { historyReadStatus: "unavailable", historyItems: [], partial: true };
    expect(preserveClipboardScanAfterFailedRefresh(scan, failed)).toBe(true);
    expect(clipboardRefreshNotice(scan, failed)).toContain("earlier scan");
    expect(getSafeCleanupPreview("clipboardHistory", scan)[0].facts).toContain("Preview: previously observed");
    expect(preserveClipboardScanAfterFailedRefresh(scan, { historyReadStatus: "complete", historyItems: [] })).toBe(false);
    expect(preserveClipboardScanAfterFailedRefresh(scan, { ...failed, historyItems: [{ preview: "new observation" }] })).toBe(false);
    expect(preserveClipboardScanAfterFailedRefresh({ historyItems: [] }, failed)).toBe(false);
  });
  test("keeps binary array details in one column without dropping any bytes", () => {
    const proxy = ALL_CATEGORIES.find(category => category.id === "proxyCache")!;
    const bytes = Array.from({ length: 512 }, (_, index) => index % 256);
    const data = { total: 1, files: [{ name: "Connection settings", value: bytes, valueType: "Binary" }] };
    const preview = getSafeCleanupPreview("proxyCache", data);
    expect(preview[0].facts).toContain(`Value: ${JSON.stringify(bytes)}`);
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={proxy} mode="advanced" count={1} items={[]} rawData={data} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain(JSON.stringify(bytes));
    expect(html).toContain("<th scope=\"col\">Value</th>");
    expect(html).not.toContain("Value 512</th>");
  });
  test("shows observed account details and source facts even when the scan is incomplete", () => {
    const result = readCleanupUserSummary({ count: 3, partial: true, records: [{ name: 'Observed file' }], scanFacts: { path: 'C:\\example' } });
    const category = ALL_CATEGORIES.find(item => item.id === 'crashDumps')!;
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={category} mode="advanced" count={-1} items={[]} groupedItems={[{ title: 'Account A', count: result.count, items: result.items, rawData: result.raw }]} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain('Observed file');
    expect(html).toContain('C:\\example');
    expect(html).toContain('could not be checked completely');
    expect(html).toContain('Unavailable');
  });
  test("keeps a captured Wi-Fi password when a later lookup cannot verify it", () => {
    const profile = { name: "Example", interfaceId: "adapter-1", password: "captured" };
    expect(wifiPasswordForProfile(profile, [{ name: "Example", interfaceId: "adapter-1", passwordStatus: "unavailable" }], false)).toBe("captured");
    expect(wifiPasswordForProfile(profile, [{ name: "Example", interfaceId: "adapter-2", passwordStatus: "available", password: "different adapter" }], false)).toBe("captured");
    expect(wifiPasswordForProfile(profile, [{ name: "Example", interfaceId: "adapter-1", passwordStatus: "available", password: "refreshed" }], false)).toBe("refreshed");
    expect(wifiPasswordForProfile({ name: "Example" }, [], false)).toBe("Unavailable");
  });

  test("shows additional Wi-Fi profiles without adding them to the clearable total", () => {
    const wifi = ALL_CATEGORIES.find(category => category.id === "wlanProfiles")!;
    const data = { total: 0, profiles: [], additionalProfiles: [{ name: "Policy network", interfaceId: "adapter-1", profileType: "Group policy", password: "example" }] };
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={wifi} mode="advanced" count={0} items={[]} rawData={data} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain("Policy network");
    expect(html).toContain("Group policy");
    expect(html).toContain("example");
    expect(html).toContain("scan total and Clear action cover only all-user profiles");
  });

  test("shows descriptive scan coverage in advanced details", () => {
    const srum = ALL_CATEGORIES.find(category => category.id === "srumData")!;
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={srum} mode="advanced" count={0} items={[]} rawData={{ files: [], coverage: "Database metadata only; historical records are not decoded." }} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain("Database metadata only; historical records are not decoded.");
    expect(html).not.toContain("<dt>Scan coverage</dt><dd>Not reported</dd>");
  });

  test("reassembles a large record without breaking Unicode across chunk boundaries", () => {
    const record = { command: `${"a".repeat(8 * 1024 * 1024)}🧪世界` };
    const bytes = new TextEncoder().encode(JSON.stringify(record));
    const encode = (value: Uint8Array) => {
      let binary = "";
      for (let index = 0; index < value.length; index += 32768) binary += String.fromCharCode(...value.subarray(index, index + 32768));
      return btoa(binary);
    };
    const boundaries = [0, bytes.length - 7, bytes.length - 4, bytes.length];
    const encoded = [0, 3 * 1024 * 1024, 6 * 1024 * 1024, boundaries[1], boundaries[2], boundaries[3]];
    const decoded = encoded.slice(0, -1).map((offset, index) => {
      const next = encoded[index + 1];
      return decodeCleanupRecordChunk({ offset, totalBytes: bytes.length, data: encode(bytes.subarray(offset, next)), nextByteOffset: next === bytes.length ? null : next }, offset)!;
    });
    expect(assembleCleanupRecordChunks(decoded, bytes.length)).toEqual(record);
    expect(assembleCleanupRecordChunks(decoded.slice(1), bytes.length)).toBeNull();
    expect(decodeCleanupRecordChunk({ offset: 0, totalBytes: 4, data: "YQ==", nextByteOffset: 2 }, 0)).toBeNull();
  });

  test("accepts a chunk only for the requested account and unchanged snapshot", () => {
    const hash = `sha256:${"a".repeat(64)}`;
    const chunk = { offset: 0, totalBytes: 2, data: "e30=", nextByteOffset: null };
    const response = { users: [{ username: "Alice", categories: [{ id: "psHistory", count: 1, items: [], records: [], sampleCount: 0, previewLimit: 20, truncated: true, sourceTruncated: false, snapshotHash: hash, nextOffset: null, totalRows: 1, recordChunk: chunk }] }] };
    expect(readExpandedCleanupSummary(response, "Alice", "psHistory")?.chunkBytes).toEqual(new Uint8Array([123, 125]));
    expect(readExpandedCleanupSummary(response, "Bob", "psHistory")).toBeNull();
    expect(readExpandedCleanupSummary(response, "Alice", "psHistory", 0, `sha256:${"b".repeat(64)}`)).toBeNull();
    expect(readExpandedCleanupSummary(response, "Alice", "psHistory", 0, hash, 1)).toBeNull();
  });
  test("accepts a page of grouped user activity without requiring it to contain every group", () => {
    const hash = `sha256:${"a".repeat(64)}`;
    const page = { users: [{ username: "Alice", categories: [{ id: "ntUserTraces", count: 3, items: [], records: [{ name: "TypedPaths", count: 1, entries: [{ value: "C:\\Example" }] }], sampleCount: 1, previewLimit: 5000, truncated: true, snapshotHash: hash, nextOffset: null, totalRows: 2 }] }] };
    expect(readExpandedCleanupSummary(page, "Alice", "ntUserTraces", 1, hash)?.count).toBe(3);
    const first = { records: [{ name: "RunMRU", count: 2 }], truncated: true, expectedActivityCount: 3 };
    const last = { records: [{ name: "TypedPaths", count: 1 }], truncated: false, expectedActivityCount: 3 };
    expect(mergeExpandedCleanupPage(first, last, 1)?.records).toHaveLength(2);
    expect(mergeExpandedCleanupPage(first, { ...last, expectedActivityCount: 4 }, 1)).toBeNull();
    expect(mergeExpandedCleanupPage(first, { ...last, records: [{ count: -1 }] }, 1)).toBeNull();
    const flat = { users: [{ username: "Alice", categories: [{ id: "ntUserTraces", count: 3, items: [], records: [{ entry: "TypedPaths: C:\\Example" }], sampleCount: 1, previewLimit: 5000, truncated: true, snapshotHash: hash, nextOffset: null, totalRows: 2 }] }] };
    expect(readExpandedCleanupSummary(flat, "Alice", "ntUserTraces", 1, hash)?.count).toBe(3);
    const malformed = { ...flat, users: [{ ...flat.users[0], categories: [{ ...flat.users[0].categories[0], records: [null] as unknown as Array<Record<string, unknown>> }] }] };
    expect(readExpandedCleanupSummary(malformed, "Alice", "ntUserTraces", 1, hash)).toBeNull();
  });
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
      { name: "History", profilePath: "C:\\Browser\\Profile 1", customDetail: "first-only" },
      { name: "Cookies", profilePath: "C:\\Browser\\Profile 2", customDetail: "second-only" },
    ] }] });
    expect(rows[0].facts.join(" ")).toContain("Profile 1");
    expect(rows[0].facts.join(" ")).not.toContain("Profile 2");
    expect(rows[0].facts.join(" ")).toContain("first-only");
    expect(rows[0].facts.join(" ")).not.toContain("second-only");
    expect(rows[1].facts.join(" ")).toContain("Profile 2");
    expect(rows[1].facts.join(" ")).toContain("second-only");
  });

  test("shows the activity record returned by the scan", () => {
    const history = ALL_CATEGORIES.find(category => category.id === "ntUserTraces")!;
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={history} mode="advanced" count={1} items={[]} rawData={{ total: 1, sections: [{ name: "RunMRU", count: 1, entries: [{ key: "a", value: "C:\\Private\\secret.exe" }] }] }} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain("Run command");
    expect(html).toContain("secret.exe");
  });

  test("shows readable search history and preserves its registry bytes", () => {
    const rows = getSafeCleanupPreview("ntUserTraces", { sections: [{ name: "WordWheelQuery", entries: [
      { key: "0", value: [99, 0, 97, 0, 102, 0, 233, 0, 0, 0] },
      { key: "MRUListEx", value: [0, 0, 0, 0, 255, 255, 255, 255] },
      { key: "1", value: [255, 216] },
      "plain activity",
    ] }] });
    expect(rows[0].facts).toContain("Value: café");
    expect(rows[0].facts).toContain("Raw Bytes: 63 00 61 00 66 00 e9 00 00 00");
    expect(rows[1].facts).toContain("Value: 00 00 00 00 ff ff ff ff");
    expect(rows[2].facts).toContain("Value: ff d8");
    expect(rows[3].facts).toContain("Value: plain activity");
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

  test("allows snapshot-checked pages of incomplete account observations without accepting a complete scan", () => {
    const preview = readCleanupUserSummary({ count: 2, partial: true, records: [{ command: "first" }], truncated: true });
    expect(preview.count).toBe(-1);
    const history = ALL_CATEGORIES.find(category => category.id === "psHistory")!;
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={history} mode="simple" count={-1} items={[]}
      groupedItems={[{ title: "Other user", username: "other-folder", count: preview.count, items: preview.items, rawData: preview.raw }]}
      clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain("Load more account details");
    const category = { id: "psHistory", count: 2, items: [], records: [{ command: "first" }], sampleCount: 1, previewLimit: 5000, truncated: true, partial: true, status: "partial", snapshotHash: `sha256:${"a".repeat(64)}`, nextOffset: 1, totalRows: 2 };
    const data = { users: [{ username: "other-folder", categories: [category] }] };
    const page = readExpandedCleanupSummary(data, "other-folder", "psHistory");
    expect(page?.partial).toBe(true);
    expect(page?.raw.partial).toBe(true);
    expect(readExpandedCleanupSummary(data, "other-folder", "psHistory", 0, `sha256:${"b".repeat(64)}`)).toBeNull();
    expect(readExpandedCleanupSummary({ users: [{ username: "other-folder", categories: [{ ...category, error: true }] }] }, "other-folder", "psHistory")).toBeNull();
  });

  test("offers more account details when an incomplete preview has no rows", () => {
    const preview = readCleanupUserSummary({ count: 2, partial: true, records: [], sampleCount: 0, previewLimit: 0, truncated: true });
    expect(preview.count).toBe(-1);
    expect(preview.raw).toMatchObject({ records: [], partial: true, truncated: true });
    const history = ALL_CATEGORIES.find(category => category.id === "psHistory")!;
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={history} mode="simple" count={-1} items={[]}
      groupedItems={[{ title: "Other user", username: "other-folder", count: preview.count, items: preview.items, rawData: preview.raw }]}
      clearing={false} clearLabel="Clear" onClose={() => {}} onClear={() => {}} />);
    expect(html).toContain("Load more account details");
    expect(html).toContain("could not be checked completely");
    expect(html).toContain("disabled");
  });

  test("offers more details for a selected account with an empty incomplete preview", () => {
    const preview = readCleanupUserSummary({ count: 2, partial: true, records: [], sampleCount: 0, previewLimit: 0, truncated: true });
    const history = ALL_CATEGORIES.find(category => category.id === "psHistory")!;
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={history} mode="simple" count={preview.count} items={preview.items}
      rawData={preview.raw} error={preview.error} accountLabel="Other user" accountName="other-folder"
      clearing={false} clearLabel="Clear" onClose={() => {}} onClear={() => {}} />);
    expect(html).toContain("Load more account details");
    expect(html).toContain("disabled");
  });

  test("accepts expanded records only from the requested account and card", () => {
    const category = { id: "psHistory", count: 2, items: ["one", "two"], records: [{ command: "one" }, { command: "two" }], sampleCount: 2, previewLimit: 50000, truncated: false, snapshotHash: `sha256:${"a".repeat(64)}`, nextOffset: null, totalRows: 2 };
    const data = { users: [{ username: "folder-name", categories: [category] }] };
    expect(readExpandedCleanupSummary(data, "folder-name", "psHistory")?.count).toBe(2);
    const withSid = { users: [{ ...data.users[0], sid: "S-1-5-21-1-2-3-1001" }] };
    expect(readExpandedCleanupSummary(withSid, "S-1-5-21-1-2-3-1001", "psHistory")?.count).toBe(2);
    expect(readExpandedCleanupSummary(withSid, "S-1-5-21-1-2-3-1002", "psHistory")).toBeNull();
    expect(readExpandedCleanupSummary(data, "S-1-5-21-1-2-3-1001", "psHistory")).toBeNull();
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

  test("decodes flat account search history while preserving the original bytes", () => {
    const entry = { section: "WordWheelQuery", key: "0", value: [99, 0, 97, 0, 102, 0, 233, 0, 0, 0] };
    for (const data of [{ records: [entry] }, { records: [{ name: "TypedPaths", entries: [{ value: "C:\\Grouped" }] }, entry] }]) {
      const rows = getSafeCleanupPreview("ntUserTraces", data);
      const row = rows[rows.length - 1];
      expect(row.facts).toContain("Value: café");
      expect(row.facts).toContain("Raw Bytes: 63 00 61 00 66 00 e9 00 00 00");
      expect(row.facts).toContain("Section: WordWheelQuery");
    }
  });

  test("preserves flat activity observations alongside grouped entries in an incomplete scan", () => {
    const activity = ALL_CATEGORIES.find(category => category.id === "ntUserTraces")!;
    const raw = { partial: true, records: [{ name: "TypedPaths", entries: [{ value: "C:\\Grouped" }] }, { entry: "C:\\Flat" }] };
    const detail = renderToStaticMarkup(<CleanupDisplayDetails category={activity} mode="advanced" count={-1} items={[]} rawData={raw} error="Incomplete" clearing={false} clearLabel="Clear" onClose={() => {}} onClear={() => {}} />);
    expect(detail).toContain("C:\\Grouped");
    expect(detail).toContain("C:\\Flat");
    expect(detail).toContain("Scan incomplete");
    expect(detail).toContain("disabled");
  });

  test("keeps source identifiers and extra fields on grouped records", () => {
    const [application] = getSafeCleanupPreview("amcache", { entries: [{ category: "NewInventoryType", count: 1, location: "inventory-key", sample: [{ name: "example" }] }] });
    expect(application.facts.join(" ")).toContain("Category source: NewInventoryType");
    expect(application.facts.join(" ")).toContain("inventory-key");
    const [activity] = getSafeCleanupPreview("ntUserTraces", { sections: [{ name: "NewActivityType", count: 1, sourcePath: "activity-key", entries: [{ value: "example" }] }] });
    expect(activity.facts.join(" ")).toContain("Section source: NewActivityType");
    expect(activity.facts.join(" ")).toContain("activity-key");
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
