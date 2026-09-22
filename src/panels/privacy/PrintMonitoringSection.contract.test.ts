import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

const componentPath = "src/panels/privacy/PrintMonitoringSection.tsx";

describe("Print Monitoring contract", () => {
  test("replaces the separate print cards in the Privacy Monitor", async () => {
    const [component, privacyIndex] = await Promise.all([
      Bun.file(componentPath).text(),
      Bun.file("src/panels/privacy/index.tsx").text(),
    ]);

    expect(component).toContain('title="Print Monitoring"');
    expect(component).toContain("Local Print Record");
    expect(component).toContain("Fleet-safe Print Signals");
    expect(privacyIndex).toContain('import PrintMonitoringSection from "./PrintMonitoringSection"');
    expect(privacyIndex).toContain("<PrintMonitoringSection />");
    expect(privacyIndex).not.toContain('import PrintActivitySection from "./PrintActivitySection"');
    expect(privacyIndex).not.toContain('import ArgusPrintUsbSection from "./ArgusPrintUsbSection"');
  });

  test("local records use only the existing Windows Print Service audit commands", async () => {
    const [component, client] = await Promise.all([
      Bun.file(componentPath).text(),
      Bun.file("src/hooks/usePrintAudit.ts").text(),
    ]);

    expect(component).toContain("printAudit.status()");
    expect(component).toContain("printAudit.recent(50)");
    expect(component).toContain("printAudit.setEnabled(enabled)");
    expect(client).toContain('invoke<PrintAuditStatus>("get_print_audit_status")');
    expect(client).toContain('invoke<PrintAuditEntry[]>("get_print_audit_log"');
    expect(client).toContain('invoke("set_print_audit_enabled"');
    expect(component).toContain("Microsoft-Windows-PrintService/Operational");
    expect(component).toContain("Event 307");
    expect(component).toContain("administrator approval once");
    expect(client).toContain('document?: string | null');
    expect(client).toContain('printer?: string | null');
    expect(client).toContain('user?: string | null');
    expect(client).toContain('jobStatus?: string | null');
  });

  test("Fleet-safe projection whitelists aggregate fields and excludes local metadata", async () => {
    const component = await Bun.file(componentPath).text();
    const projection = component.split("export function toFleetSafePrintSignals")[1]?.split("function InfoButton")[0] ?? "";

    expect(projection).toContain('entry.kind === "print"');
    expect(projection).toContain('entry.class === "print_job"');
    expect(projection).toContain("windowStart");
    expect(projection).toContain("windowEnd");
    expect(projection).toContain("magnitude");
    expect(projection).toContain("severity");
    expect(projection).not.toContain("document");
    expect(projection).not.toContain("printer");
    expect(projection).not.toContain("user");
    expect(projection).not.toContain("path");
    expect(projection).not.toContain("content");

    expect(component).toContain("Document names, printer names, usernames, paths, and document contents are never sent to Fleet.");
  });

  test("does not expose unfinished watermarking controls in Privacy Monitor", async () => {
    const component = await Bun.file(componentPath).text();

    expect(component).not.toContain('data-testid="print-watermarking"');
    expect(component).not.toContain('About print watermarking');
    expect(component).not.toContain('Fleet-managed controlled-PDF workflow');
  });

  test("info buttons support hover, keyboard, click, Escape, and touch", async () => {
    const [component, privacyCss] = await Promise.all([
      Bun.file(componentPath).text(),
      Bun.file("src/panels/privacy/index.css").text(),
    ]);

    expect(component).toContain("onMouseEnter={() => setOpen(true)}");
    expect(component).toContain("onFocus={() => setOpen(true)}");
    expect(component).toContain("onClick={(event) =>");
    expect(component).toContain('event.key === "Escape"');
    expect(component).toContain('event.pointerType !== "touch"');
    expect(component).toContain("aria-expanded={open}");
    expect(component).toContain('role="tooltip"');
    expect(component).toContain('role="alert"');
    expect(privacyCss).toContain('[role="tooltip"]');
    expect(privacyCss).toContain('background: var(--color-bg-secondary) !important');
    expect(privacyCss).toContain('opacity: 1');
  });

  test("paid boundary remains explicit", async () => {
    const component = await Bun.file(componentPath).text();

    expect(component).toContain('const paid = canUse("paid")');
    expect(component).toContain("require WinCommander Pro");
  });
});
