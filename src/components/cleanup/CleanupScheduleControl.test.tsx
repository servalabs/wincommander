import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ALL_CATEGORIES } from "../../panels/cleanup/cleanupCategories";
import CleanupScheduleControl from "./CleanupScheduleControl";
import CleanupTraceCard from "./CleanupTraceCard";

describe("individual cleanup schedule controls", () => {
  test("offers management of an existing blocked schedule without offering a new schedule", () => {
    const turnOff = () => true;
    const control = renderToStaticMarkup(<CleanupScheduleControl categoryLabel="Wi-Fi Profiles" minInterval={60} scheduleMinutes={60} schedulePayloadVerified={true} onClearSchedule={turnOff} />);
    expect(control).toContain("Manage Wi-Fi Profiles scheduled wipe");
    expect(control).toContain("1h");
    expect(control).not.toContain("Schedule a wipe for");
    expect(control).not.toContain("Change Wi-Fi Profiles scheduled wipe");

    const category = { ...ALL_CATEGORIES.find(item => item.id === "wlanProfiles")!, schedulable: false };
    const card = renderToStaticMarkup(<CleanupTraceCard category={category} count={-1} preview={[]} loading={false} clearing={false} onClear={() => {}} scheduleMinutes={60} schedulePayloadVerified={true} onClearSchedule={turnOff} />);
    expect(card).toContain("Manage Wi-Fi Profiles scheduled wipe");
  });

  test("offers removal while warning that an existing task payload is unverified", () => {
    const markup = renderToStaticMarkup(<CleanupScheduleControl categoryLabel="Wi-Fi Profiles" minInterval={60} scheduleMinutes={60} schedulePayloadVerified={false} onClearSchedule={() => true} />);
    expect(markup).toContain("cleanup script could not be verified");
    expect(markup).toContain("Manage Wi-Fi Profiles scheduled wipe; cleanup script not verified");
  });

  test("does not show a schedule control from a clear callback alone", () => {
    const control = renderToStaticMarkup(<CleanupScheduleControl categoryLabel="Wi-Fi Profiles" minInterval={60} onClearSchedule={() => true} />);
    expect(control).toBe("");
  });

  test("shows a verified disabled schedule without claiming it will run", () => {
    const html = renderToStaticMarkup(<CleanupScheduleControl categoryLabel="Wi-Fi Profiles" minInterval={60} scheduleDisabled schedulePayloadVerified={true} onClearSchedule={() => true} />);
    expect(html).toContain("Manage disabled Wi-Fi Profiles scheduled wipe");
    expect(html).toContain("OFF");
    expect(html).not.toContain("Schedule a wipe for");
  });
});
