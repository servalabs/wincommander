import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { FleetSignedContact } from "./FleetSignedContact";

describe("signed Fleet contact observation", () => {
  test("shows the recorded device time as historical rather than current health", () => {
    const timestamp = "2026-11-03T12:00:00Z";
    const html = renderToStaticMarkup(<FleetSignedContact timestamp={timestamp} />);
    expect(html).toContain(`dateTime="${timestamp}"`);
    expect(html).toContain(new Date(timestamp).toLocaleString());
    expect(html).toContain("Historical time from this device");
    expect(html).toContain("does not indicate the current connection");
  });

  test("does not present absent or malformed observations as verified", () => {
    for (const timestamp of [undefined, null, "", "not-a-date", {}, 42]) {
      const html = renderToStaticMarkup(<FleetSignedContact timestamp={timestamp} />);
      expect(html).toContain("Verified Fleet contact is unavailable");
      expect(html).not.toContain("<time");
    }
  });
});
