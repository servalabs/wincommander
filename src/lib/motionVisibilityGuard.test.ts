import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const indexHtml = readFileSync("index.html", "utf8");
const themeCss = readFileSync("src/styles/v2-theme.css", "utf8");
const dashboardCss = readFileSync("src/panels/dashboard/index.css", "utf8");
const radarCss = readFileSync("src/components/dashboard/SovereigntyRadar.css", "utf8");

describe("decorative motion visibility guards", () => {
  test("installs one document visibility marker before React mounts", () => {
    expect(indexHtml).toContain('toggleAttribute("data-page-hidden", document.hidden)');
    expect(indexHtml).toContain('addEventListener("visibilitychange", syncPageVisibility)');
  });

  test("pauses shared and dashboard loops while hidden", () => {
    expect(themeCss).toContain("html[data-page-hidden] .wc-scan-bar > i");
    expect(themeCss).toContain("transform: translateX(250%)");
    expect(dashboardCss).toContain("html[data-page-hidden] .dashboard-panel .spin");
    expect(radarCss).toContain("html[data-page-hidden] .sov-radar__sweep");
  });
});
