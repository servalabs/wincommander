import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import NeedsAttention from "./NeedsAttention";
import type { ScanFinding } from "../startup/WizardAnimations";

const finding: ScanFinding = { id: "officeLog", label: "Disable Office logging", category: "privacy", severity: "warning", impact: "Logging policy needs attention" };

test("unverified fixes stay actionable without error logs inside the finding list", () => {
  const html = renderToStaticMarkup(<NeedsAttention findings={[finding]} busyIds={new Set()} onFixOne={() => undefined} onFixAll={() => undefined} onIgnore={() => undefined} />);
  expect(html).not.toContain('role="alert"');
  expect(html).toContain("Disable Office logging");
  expect(html).toContain(">Fix</button>");
});

test("ignored findings are counted as ignored rather than announced as fixed", () => {
  const html = renderToStaticMarkup(<NeedsAttention findings={[]} busyIds={new Set()} ignoredFindingIds={[finding.id]} knownFindings={[finding]} onRestoreIgnored={() => undefined} onFixOne={() => undefined} onFixAll={() => undefined} onIgnore={() => undefined} />);
  expect(html).toContain("Ignored (1)");
  expect(html).not.toContain("fixed");
});
