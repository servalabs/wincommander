import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import StartupRecovery from "./StartupRecovery";

describe("startup recovery", () => {
  test("shows an accessible settings failure and a recovery action", () => {
    const markup = renderToStaticMarkup(
      <StartupRecovery error="Settings could not be loaded." onRetry={() => {}} />,
    );
    expect(markup).toContain('role="alert"');
    expect(markup).toContain("Settings could not be loaded.");
    expect(markup).toContain('type="button"');
    expect(markup).toContain("Retry startup");
  });

  test("removes the error and retry action while startup is being retried", () => {
    expect(renderToStaticMarkup(<StartupRecovery error={null} onRetry={() => {}} />)).toBe("");
  });
});
