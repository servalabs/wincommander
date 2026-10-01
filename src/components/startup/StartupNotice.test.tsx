import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import StartupNotice from "./StartupNotice";

describe("startup slow-read notice", () => {
  test("keeps a slow settings read nonfatal while offering manual retry", () => {
    const markup = renderToStaticMarkup(
      <StartupNotice message="Still opening saved settings." onRetry={() => {}} />,
    );
    expect(markup).toContain('role="status"');
    expect(markup).not.toContain('role="alert"');
    expect(markup).toContain("Still opening saved settings.");
    expect(markup).toContain("Retry now");
  });

  test("does not render a status after settings are available", () => {
    expect(renderToStaticMarkup(<StartupNotice message={null} onRetry={() => {}} />)).toBe("");
  });
});
