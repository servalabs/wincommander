import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

function panelSource(path: string) {
  const sourcePath = new URL(`./${path}`, import.meta.url).pathname.replace(
    /^\/([A-Za-z]:\/)/,
    "$1",
  );
  return readFileSync(sourcePath, "utf8");
}

describe("assigned panel accessibility contracts", () => {
  test("names repeated file-search actions with their result", () => {
    const names = panelSource("search-files/NameResultsSection.tsx");
    const content = panelSource("search-files/ContentResultsSection.tsx");

    expect(names).toContain("aria-label={`Open ${r.name}`}");
    expect(names).toContain("aria-label={`Open the containing folder for ${r.name}`}");
    expect(content).toContain("aria-label={`Copy the full path for ${row.name}`}");
    expect(content).toContain('className="sfp-doc-expand-error" role="alert"');
  });

  test("keeps vault controls and validation messages accessible", () => {
    const vault = panelSource("vault/index.tsx");
    const wizard = panelSource("vault/CreateVolumeWizard.tsx");

    expect(vault).toContain('aria-label="Browse for an encrypted volume"');
    expect(vault).toContain('aria-label={showPassword ? "Hide volume password" : "Show volume password"}');
    expect(wizard).toContain('aria-label="Browse for a container folder"');
    expect(wizard).toContain('className="pw-mismatch" role="alert"');
    expect(wizard).toContain('aria-pressed={filesystem === f.value}');
  });

  test("announces errors and exposes stateful controls across operational panels", () => {
    const mesh = panelSource("mesh/index.tsx");
    const fleet = panelSource("fleet/FleetConnectView.tsx");
    const advisor = panelSource("advisor/index.tsx");
    const flows = panelSource("flows/index.tsx");

    expect(mesh).toContain('role="alert" className="p-3 bg-amber-500/10');
    expect(mesh).toContain('aria-pressed={staging.allowLanAccess}');
    expect(fleet.match(/className="fleet-connect-error" role="alert"/g)?.length).toBeGreaterThanOrEqual(5);
    expect(advisor).toContain('className="advisor-error" role="alert"');
    expect(advisor).toContain('aria-label={`Downloading ${model}`}');
    expect(flows).toContain('aria-label={`Run ${rule.name} now`}');
  });

  test("keeps empty server-apps state actionable and controls explicitly named", () => {
    const apps = panelSource("server-apps/index.tsx");
    const dialog = panelSource("server-apps/ManageAppsDialog.tsx");
    const productivity = panelSource("productivity/index.tsx");

    expect(apps).toContain('action={<Button onClick={() => setManageOpen(true)}>Manage Server Apps</Button>}');
    expect(apps).toContain('role="tablist" aria-label="Server applications"');
    expect(dialog).toContain('aria-label={`Remove ${row.name}`}');
    expect(productivity).not.toContain('<a onClick=');
  });
});
