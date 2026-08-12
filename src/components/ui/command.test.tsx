import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import {
  CommandDialog,
  CommandEmpty,
  CommandInput,
  CommandList,
} from "./command";

describe("CommandDialog", () => {
  test("renders an accessible Radix dialog description", () => {
    const html = renderToStaticMarkup(
      <CommandDialog open description="Search every available command">
        <CommandInput />
        <CommandList>
          <CommandEmpty>No results</CommandEmpty>
        </CommandList>
      </CommandDialog>,
    );

    expect(html).toContain('data-slot="dialog-description"');
    expect(html).toContain("Search every available command");
  });
});
