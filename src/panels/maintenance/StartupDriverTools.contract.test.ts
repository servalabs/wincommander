import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("System Managers compact detail views", () => {
  test("keeps the startup impact review folded until requested", async () => {
    const source = await Bun.file("src/panels/maintenance/StartupDriverTools.tsx").text();

    expect(source).toContain('const [isStartupImpactOpen, setIsStartupImpactOpen] = useState(false)');
    expect(source).toContain('aria-expanded={isStartupImpactOpen}');
    expect(source).toContain('{isStartupImpactOpen && <div className="startup-impact-section__list">');
    expect(source).toContain('className="startup-impact-row"');
  });

  test("places meaningful task authors in the title row and omits blank scheduler placeholders", async () => {
    const source = await Bun.file("src/components/tweaks/managers/ScheduledTasksManager.tsx").text();

    expect(source).toContain('function hasTaskAuthor(author: string | null): author is string');
    expect(source).toContain('function hasTaskPath(path: string): boolean');
    expect(source).toContain('normalized !== "\\\\"');
    expect(source).toContain('className="scheduled-tasks-manager__meta"');
    expect(source).toContain('className="scheduled-tasks-manager__author"');
    expect(source).not.toContain('{t.Author && <span>Author: {t.Author}</span>}');
  });

  test("keeps an expanded startup impact review bounded to a scrollable list", async () => {
    const css = await Bun.file("src/panels/maintenance/StartupDriverTools.css").text();

    expect(css).toContain('.startup-impact-section__list');
    expect(css).toContain('max-height: min(480px, calc(100vh - 360px))');
    expect(css).toContain('overflow-y: auto');
  });

  test("folds long concealment signals into compact, titled tags", async () => {
    const [source, css] = await Promise.all([
      Bun.file("src/panels/runtime-visibility/index.tsx").text(),
      Bun.file("src/panels/runtime-visibility/index.css").text(),
    ]);

    expect(source).toContain('r.tags.slice(0, 2)');
    expect(source).toContain('r.tags.length > 2');
    expect(css).toContain('max-height: 48px');
    expect(css).toContain('align-items: flex-start');
    expect(css).toContain('text-overflow: ellipsis');
  });
});
