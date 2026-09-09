import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ThemeProvider } from "./ThemeContext";

describe("startup theme", () => {
  test("paints startup content before settings IPC resolves", () => {
    // Server rendering never runs effects or resolves the settings request.
    const markup = renderToStaticMarkup(
      <ThemeProvider><div role="status">Starting WinCommander</div></ThemeProvider>,
    );
    expect(markup).toContain('role="status"');
    expect(markup).toContain("Starting WinCommander");
  });
});
