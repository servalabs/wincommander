import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import VaultAccessPatternPicker from "./VaultAccessPatternPicker";

describe("Vault access pattern selector", () => {
  test("keeps Share, View only, and Edit choices as three distinct values", () => {
    const html = renderToStaticMarkup(<VaultAccessPatternPicker value="shared-write" onChange={() => undefined} />);

    expect(html).toContain('role="radiogroup" aria-label="Vault access pattern"');
    expect(html).toContain('data-vault-access-preset="private"');
    expect(html).toContain('data-vault-access-preset="shared-read"');
    expect(html).toContain('data-vault-access-preset="shared-write"');
    expect(html).toMatch(/role="radio" aria-checked="true" data-vault-access-preset="shared-write"/);
    expect(html).toMatch(/role="radio" aria-checked="false" data-vault-access-preset="shared-read"/);
  });
});
