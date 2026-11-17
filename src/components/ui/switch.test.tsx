import { describe, expect, test } from "bun:test";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Switch } from "./switch";
import { AppProvider } from "../../context/AppContext";
import { AuthModeProvider } from "../../context/AuthModeContext";
import { LiveMetricsProvider } from "../../context/LiveMetricsContext";
import { MotionPreferenceProvider } from "../../hooks/useMotionPreference";

// Switch reads useMotionPreference(), which reads AuthMode and opportunistically
// uses AppContext when the full desktop provider tree is available.
function renderWithProviders(ui: ReactElement): string {
  const queryClient = new QueryClient();
  return renderToStaticMarkup(
    <QueryClientProvider client={queryClient}>
      <AuthModeProvider>
        <LiveMetricsProvider>
          <AppProvider>
            <MotionPreferenceProvider>{ui}</MotionPreferenceProvider>
          </AppProvider>
        </LiveMetricsProvider>
      </AuthModeProvider>
    </QueryClientProvider>
  );
}

describe("Switch", () => {
  test("uses explicit off-state contrast tokens", () => {
    const html = renderWithProviders(<Switch checked={false} />);

    expect(html).toContain("--switch-off-bg");
    expect(html).toContain("--switch-off-border");
    expect(html).toContain("--switch-off-thumb");
  });

  test("remains renderable across a presentation-only provider refresh", () => {
    const html = renderToStaticMarkup(
      <AuthModeProvider>
        <MotionPreferenceProvider><Switch checked={false} /></MotionPreferenceProvider>
      </AuthModeProvider>,
    );

    expect(html).toContain("--switch-off-bg");
  });

  test("exposes switch semantics and the accessible label", () => {
    const html = renderWithProviders(<Switch checked aria-label="Encrypted DNS" />);

    expect(html).toContain('role="switch"');
    expect(html).toContain('aria-checked="true"');
    expect(html).toContain('aria-label="Encrypted DNS"');
    expect(html).toContain('data-state="checked"');
  });

  test("is a native button so Space/Enter activate it", () => {
    const html = renderWithProviders(<Switch checked={false} disabled />);

    expect(html).toMatch(/^<button type="button"/);
    expect(html).toContain("disabled");
    expect(html).toContain('aria-checked="false"');
  });

  test("size sm renders the compact track", () => {
    const md = renderWithProviders(<Switch checked={false} />);
    const sm = renderWithProviders(<Switch checked={false} size="sm" />);

    expect(md).toContain("w-[46px]");
    expect(sm).toContain("w-[38px]");
    expect(sm).not.toContain("w-[46px]");
  });
});
