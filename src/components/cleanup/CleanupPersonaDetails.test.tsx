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
      expect(html).not.toContain("file.txt");
    }
    expect(casual).not.toContain("Scan coverage");
    expect(secure).toContain("Scan coverage");
    expect(secure).toContain("Record categories");
  });

  test("offers useful browser and Wi-Fi previews without private values", () => {
    const browser = ALL_CATEGORIES.find(category => category.id === "browserFootprints")!;
    const wifi = ALL_CATEGORIES.find(category => category.id === "wlanProfiles")!;
    const browserHtml = renderToStaticMarkup(<CleanupDisplayDetails category={browser} mode="advanced" count={1} items={[]} rawData={{ browsers: [{ browser: "Google Chrome", profilePath: "C:\\Private\\Profile", totalSizeKB: 2, artifacts: [{ name: "History", sizeKB: 1 }] }] }} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(browserHtml).toContain("Google Chrome");
    expect(browserHtml).toContain("History: 1.0 KB");
    expect(browserHtml).not.toContain("Private\\Profile");
    const wifiHtml = renderToStaticMarkup(<CleanupDisplayDetails category={wifi} mode="simple" count={1} items={[]} rawData={{ profiles: [{ name: "PrivateNetwork", password: "secret-value" }] }} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(wifiHtml).toContain("Saved Wi-Fi profile 1");
    expect(wifiHtml).not.toContain("PrivateNetwork");
    expect(wifiHtml).not.toContain("secret-value");
  });

  test("names activity categories without exposing the recorded command or path", () => {
    const history = ALL_CATEGORIES.find(category => category.id === "ntUserTraces")!;
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={history} mode="advanced" count={1} items={[]} rawData={{ total: 1, sections: [{ name: "RunMRU", count: 1, entries: [{ key: "a", value: "C:\\Private\\secret.exe" }] }] }} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain("Run command 1");
    expect(html).not.toContain("secret.exe");
  });

  test("projects clipboard, network drive and recycle-bin rows without private content", () => {
    const clipboard = getSafeCleanupPreview("clipboardHistory", { historyItems: [{ preview: "sensitive clipboard", type: "text" }] });
    const drives = getSafeCleanupPreview("netDrives", { drives: [{ Name: "Z:", DisplayRoot: "\\\\private-server\\secret" }] });
    const recycle = getSafeCleanupPreview("recycleBin", { items: [{ name: "private file.txt", path: "C:\\private" }] });
    expect(clipboard[0].label).toBe("Clipboard item 1");
    expect(drives[0].label).toBe("Drive Z:");
    expect(recycle[0].label).toBe("Recycle Bin record 1");
    expect(/sensitive clipboard|private-server|private file/.test(JSON.stringify([clipboard, drives, recycle]))).toBe(false);
  });

  test("distinguishes bounded preview rows from the full count", () => {
    const samples = Array.from({ length: 30 }, (_, index) => ({ name: `Sample ${index + 1}` }));
    const source = { total: 30, entries: [{ category: "InventoryApplication", count: 30, sample: samples }] };
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={amcache} mode="advanced" count={30} items={[]} rawData={source} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain("30 records");
    expect(html).toContain("20 preview rows available here (up to 20 shown)");
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
      expect(html).not.toContain("C:\\private");
    }
  });
});
