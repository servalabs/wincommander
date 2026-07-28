import { describe, expect, test } from "bun:test";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Switch } from "./switch";
import { AppProvider } from "../../context/AppContext";
import { AuthModeProvider } from "../../context/AuthModeContext";

// Switch reads useMotionPreference(), which in turn reads useAppState() (and
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

describe("Switch", () => {
  test("uses explicit off-state contrast tokens", () => {
    const html = renderWithProviders(<Switch checked={false} />);

    expect(html).toContain("--switch-off-bg");
    expect(html).toContain("--switch-off-border");
    expect(html).toContain("--switch-off-thumb");
  });
});
