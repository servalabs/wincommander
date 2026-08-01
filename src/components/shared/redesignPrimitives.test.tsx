import { describe, expect, test } from "bun:test";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import AdaptiveCard from "./AdaptiveCard";
import StatusPill from "./StatusPill";
import SystemRadar from "./SystemRadar";
import TraceDetailDialog from "./TraceDetailDialog";
import WCSwitch from "./WCSwitch";
import type { CleanupCategory } from "../../panels/cleanup/cleanupCategories";
import { AppProvider } from "../../context/AppContext";
import { AuthModeProvider } from "../../context/AuthModeContext";

// WCSwitch reads useMotionPreference(), which in turn reads useAppState() (and
// useAuthMode()) — so a static render needs the real provider stack, mirroring
// main.tsx's nesting (QueryClientProvider > AuthModeProvider > AppProvider).
function renderWithProviders(ui: ReactElement): string {
  const queryClient = new QueryClient();
  return renderToStaticMarkup(
    <QueryClientProvider client={queryClient}>
      <AuthModeProvider>
        <AppProvider>{ui}</AppProvider>
      </AuthModeProvider>
    </QueryClientProvider>
  );
}

const category: CleanupCategory = {
  id: "sample",
  label: "Sample Trace",
  description: "A sample trace category",
  icon: "folder-open",
  color: "#3b82f6",
  severity: "warning",
  group: "standard",
  getDataKey: "getSample",
  clearDataKey: "clearSample",
  extractPreview: () => ({ count: 0, items: [] }),
};

describe("redesign shared primitives", () => {
  test("adaptive card renders tile and row density variants", () => {
    const guided = renderToStaticMarkup(
      <AdaptiveCard density="guided" title="Privacy" description="Curated controls" />,
    );
    const expert = renderToStaticMarkup(
      <AdaptiveCard density="expert" title="Privacy" description="Curated controls" />,
    );

    expect(guided).toContain("adaptive-card--tile");
    expect(expert).toContain("adaptive-card--row");
  });

  test("status pill renders a stable status label", () => {
    const html = renderToStaticMarkup(<StatusPill tone="success">Ready</StatusPill>);

    expect(html).toContain("status-pill--success");
    expect(html).toContain("Ready");
  });

  test("trace detail dialog centralizes trace action state", () => {
    const html = renderToStaticMarkup(
      <TraceDetailDialog
        category={category}
        isOpen
        count={3}
        items={["one", "two"]}
        rawData={{ entries: [{ name: "one", source: "registry", modified: "today" }] }}
        clearing={false}
        onClose={() => undefined}
        onClear={() => undefined}
      />,
    );

    expect(html).toContain("Sample Trace");
    expect(html).toContain("one");
    expect(html).toContain("Source");
    expect(html).toContain("Modified");
    expect(html).toContain("Filter every field");
    expect(html).toContain("Clear");
  });

  test("system radar renders score and findings without panel-specific copy", () => {
    const html = renderToStaticMarkup(
      <SystemRadar
        score={82}
        findings={[
          { id: "privacy", label: "Privacy", status: "ready" },
          { id: "network", label: "Network", status: "attention" },
        ]}
      />,
    );

    expect(html).toContain("82");
    expect(html).toContain("Privacy");
    expect(html).toContain("Network");
  });

  test("WCSwitch renders explicit off-state contrast hooks", () => {
    const html = renderWithProviders(<WCSwitch checked={false} label="Sample" />);

    expect(html).toContain("wc-switch");
    expect(html).not.toContain("is-on");
  });
});
