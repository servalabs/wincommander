import { expect, test } from "bun:test";
import { parseRegisteredCommandEntries } from "./check-tier-drift";

function registration(p2: string, p3: string): string {
  return `pub fn register_p2_commands() { ${p2} }\npub fn register_p3_commands() { ${p3} }`;
}
function entry(name: string, tier: "Free" | "Paid"): string {
  return `CommandEntry { command: "${name}", tier: CommandTier::${tier} }`;
}

test("registered tiers follow runtime initialization order and allow identical duplicates", () => {
  const source = registration(`${entry("First", "Paid")} ${entry("First", "Paid")}`, `${entry("First", "Paid")} ${entry("Second", "Free")}`);
  expect([...parseRegisteredCommandEntries(source)]).toEqual([["First", "paid"], ["Second", "free"]]);
});

test("conflicting registrations fail the gate in either order", () => {
  for (const [first, later] of [["Free", "Paid"], ["Paid", "Free"]] as const) {
    const source = registration(entry("Example", first), entry("Example", later));
    expect(() => parseRegisteredCommandEntries(source)).toThrow(`conflicting registered tiers for Example: first ${first.toLowerCase()}, later ${later.toLowerCase()}`);
  }
});
