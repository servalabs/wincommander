import { expect, test } from "bun:test";
import { ESLint } from "eslint";
import react from "eslint-plugin-react";

test("React unsafe HTML rule retains wildcard component matching", async () => {
  const lint = new ESLint({
    overrideConfigFile: true,
    overrideConfig: [{
      files: ["**/*.jsx"],
      languageOptions: { parserOptions: { ecmaFeatures: { jsx: true } } },
      plugins: { react },
      rules: { "react/no-danger": ["error", { customComponentNames: ["Unsafe*"] }] },
    }],
  });
  const [unsafe] = await lint.lintText(
    "const x = <UnsafeBox dangerouslySetInnerHTML={{__html: input}} />;",
    { filePath: "src/test.jsx" },
  );
  const [safe] = await lint.lintText(
    "const x = <OtherBox dangerouslySetInnerHTML={{__html: input}} />;",
    { filePath: "src/test.jsx" },
  );
  expect(unsafe.messages.some(message => message.ruleId === "react/no-danger")).toBe(true);
  expect(safe.messages.some(message => message.ruleId === "react/no-danger")).toBe(false);
});
