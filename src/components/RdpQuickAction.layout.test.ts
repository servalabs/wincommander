import { describe, expect, test } from "bun:test";
import componentSource from "./RdpQuickAction.tsx?raw";
import sidebarCss from "./Sidebar.css?raw";

describe("RdpQuickAction manage dialog layout", () => {
  test("keeps endpoint action buttons compact inside list cards", () => {
    expect(componentSource).toContain("node-action-btn");
    expect(sidebarCss).toContain(".node-list-item .node-action-btn");
    expect(sidebarCss).toContain("width: 32px");
    expect(sidebarCss).toContain("min-width: 32px");
    expect(sidebarCss).toContain("padding: 0");
  });

  test("allows endpoint labels to shrink before the action buttons overflow", () => {
    expect(sidebarCss).toContain(".node-list-item .node-item-info");
    expect(sidebarCss).toContain("min-width: 0");
    expect(sidebarCss).toContain(".node-list-item .node-item-label");
    expect(sidebarCss).toContain(".node-list-item .node-item-host");
    expect(sidebarCss).toContain("text-overflow: ellipsis");
  });

  test("exposes the full endpoint name via tooltip when the sidebar dropdown row ellipsizes it", () => {
    // .endpoint-name is single-line + ellipsis (Sidebar.css), so a long
    // nickname/hostname gets visually cut off in the collapsed sidebar list.
    // The row must carry a title attribute so the full text is still reachable.
    const rowMatch = componentSource.match(/className="rdp-endpoint-item"[\s\S]*?<\/button>/);
    expect(rowMatch).not.toBeNull();
    expect(rowMatch![0]).toMatch(/title=\{node\.label\}/);
  });
});
