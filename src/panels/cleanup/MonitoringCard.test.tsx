import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import MonitoringCard from "./MonitoringCard";
import { VIEW_ONLY_CATEGORIES } from "./cleanupCategories";

const category = VIEW_ONLY_CATEGORIES[0];
const data = { count: -1, items: [], loading: false, clearing: false };

test("monitoring offers a scan before results and details after a successful scan", () => {
    const before = renderToStaticMarkup(<MonitoringCard category={category} data={data} onScan={() => {}} onViewDetails={() => {}} />);
    expect(before).toContain("Not scanned");
    expect(before).toContain(">Scan</button>");
    expect(before).not.toContain("View details");
    const after = renderToStaticMarkup(<MonitoringCard category={category} data={{ ...data, count: 1, raw: { processes: [{ pid: 42, name: "sample" }] } }} onScan={() => {}} onViewDetails={() => {}} />);
    expect(after).toContain("1 process observed");
    expect(after).toContain("View details");
    expect(after).toContain("Rescan");
});

test("monitoring does not present a partial or failed scan as complete", () => {
    const partial = renderToStaticMarkup(<MonitoringCard category={category} data={{ ...data, count: -1, raw: { status: "partial", partial: true, processes: [{ pid: 42, name: "sample" }] } }} onScan={() => {}} onViewDetails={() => {}} />);
    expect(partial).toContain("Some process details or processes could not be checked.");
    expect(partial).toContain("View details");
    const failed = renderToStaticMarkup(<MonitoringCard category={category} data={{ ...data, error: "access denied" }} onScan={() => {}} />);
    expect(failed).toContain("Scan failed");
    expect(failed).toContain("Rescan");
});
