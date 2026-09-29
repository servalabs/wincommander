import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import DriveLetterPicker from "./DriveLetterPicker";

test("an unavailable or exhausted drive list never offers invented letters", () => {
  const html = renderToStaticMarkup(<DriveLetterPicker id="letters" value="V" letters={[]} onChange={() => undefined} />);
  expect(html).toContain("No free drive letters");
  expect(html).not.toContain('role="radio"');
});

test("only letters supplied by the service can be selected", () => {
  const html = renderToStaticMarkup(<DriveLetterPicker id="letters" value="W" letters={["W", "Z"]} onChange={() => undefined} />);
  expect(html.match(/role="radio"/g)).toHaveLength(2);
  expect(html).toContain('aria-checked="true"');
  expect(html).not.toContain(">V:");
});
