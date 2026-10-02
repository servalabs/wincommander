import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import DriveLetterPicker from "./DriveLetterPicker";

test("an exhausted checked drive list never offers invented letters", () => {
  const html = renderToStaticMarkup(<DriveLetterPicker id="letters" value="V" letters={[]} onChange={() => undefined} />);
  expect(html).toContain("No free drive letters");
  expect(html).not.toContain('value="V"');
});

test("only letters supplied by the service can be selected", () => {
  const html = renderToStaticMarkup(<DriveLetterPicker id="letters" value="W" letters={["W", "Z"]} onChange={() => undefined} />);
  expect(html).toContain('<select');
  expect(html).toContain('value="W"');
  expect(html).toContain('value="Z"');
  expect(html).not.toContain(">V:");
});

test("loading and failed discovery never claim that all drive letters are occupied", () => {
  for (const state of [{ loading: true }, { unavailable: true }]) {
    const html = renderToStaticMarkup(<DriveLetterPicker id="letters" value="" letters={[]} {...state} onChange={() => undefined} />);
    expect(html).not.toContain("No free drive letters");
    expect(html).toContain("disabled");
  }
});

test("a validated selected letter is retained without an exhausted-list warning", () => {
  const html = renderToStaticMarkup(<DriveLetterPicker id="letters" value="J" letters={["J"]} onChange={() => undefined} />);
  expect(html).toContain('value="J" selected=""');
  expect(html).not.toContain("No free drive letters");
});

test("the Secure Storage cleanup action is opt-in", () => {
  const normal = renderToStaticMarkup(<DriveLetterPicker id="letters" value="W" letters={["W"]} onChange={() => undefined} />);
  const secureStorage = renderToStaticMarkup(<DriveLetterPicker id="letters" value="W" letters={["W"]} onChange={() => undefined} onReleaseOrphaned={() => undefined} />);
  expect(normal).not.toContain("Check and free unavailable Vault letters");
  expect(secureStorage).toContain("Check and free unavailable Vault letters");
});
