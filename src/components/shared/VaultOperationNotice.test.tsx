import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import VaultOperationNotice from "./VaultOperationNotice";

test("errors are visible alerts and confirmed successes use a separate status tone", () => {
  const failure = renderToStaticMarkup(<VaultOperationNotice message="Install Pro before mounting." />);
  expect(failure).toContain('role="alert"');
  const success = renderToStaticMarkup(<VaultOperationNotice message="Vault saved." tone="success" />);
  expect(success).toContain('role="status"');
  expect(success).not.toContain('role="alert"');
});
