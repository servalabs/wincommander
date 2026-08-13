import { describe, expect, test } from "bun:test";
import { applyProductAliases } from "./assets";

declare const Bun: {
  file(path: string): {
    text(): Promise<string>;
  };
};

describe("applyProductAliases", () => {
  test("keeps shared asset module URLs separate from raw WebView asset URLs", async () => {
    const source = await Bun.file("src/assets.ts").text();
    const moduleQueries = source.match(/query: "\?url&wc-module"/g) ?? [];

    // softwares, software blocklist, entities, apps, editorial + 5 product roots
    // (contingency, private-phone, private-server, theron, wincommander)
    expect(moduleQueries).toHaveLength(10);
    expect(source).not.toContain('query: "?url"');
    // Product media must load from the app's assets submodule, not a
    // missing workspace sibling at ../../assets (that left the title logo empty).
    expect(source).toContain('../assets/products/wincommander/**/*');
    expect(source).not.toContain('../../assets/products/');
  });

  test("keeps legacy product keys wired to the renamed asset files", () => {
    const aliased = applyProductAliases({
      "private-phone/phone-top-view.png": "/private-phone/phone-top-view.png",
      "private-server/pro-nobg.png": "/private-server/pro-nobg.png",
      "private-server/max-with-bg.jpg": "/private-server/max-with-bg.jpg",
      "private-server/max-nobg.png": "/private-server/max-nobg.png",
      "theron/theron-chat.png": "/theron/theron-chat.png",
      "theron/theron-entity-graph.png": "/theron/theron-entity-graph.png",
      "theron/openwebui-rag-chat-army-data-darkmode.png": "/theron/openwebui-rag-chat-army-data-darkmode.png",
      "theron/openwebui-rag-chat-army-data-lightmode.png": "/theron/openwebui-rag-chat-army-data-lightmode.png",
      "theron/ai-vision-enemy-tank-humans-detection.png": "/theron/ai-vision-enemy-tank-humans-detection.png",
      "theron/end-to-end-monitoring-system-ai-cam-ai-server.png": "/theron/end-to-end-monitoring-system-ai-cam-ai-server.png",
      "wincommander/wc-dashboard.png": "/wincommander/wc-dashboard.png",
      "wincommander/wc-dashboard-lightmode.png": "/wincommander/wc-dashboard-lightmode.png",
      "wincommander/wc-forensic-cleanup.png": "/wincommander/wc-forensic-cleanup.png",
      "wincommander/wc-privacy-settings.png": "/wincommander/wc-privacy-settings.png",
      "wincommander/wc-dashboard-with-callouts.png": "/wincommander/wc-dashboard-with-callouts.png",
      "wincommander/wc-network-control-honeypot-callout.png": "/wincommander/wc-network-control-honeypot-callout.png",
      "contingency/usb-decoy-hub.png": "/contingency/usb-decoy-hub.png",
    });

    expect(aliased["private-phone/hero.png"]).toBe("/private-phone/phone-top-view.png");
    expect(aliased["private-server/base.png"]).toBe("/private-server/pro-nobg.png");
    expect(aliased["private-server/base-model.png"]).toBe("/private-server/pro-nobg.png");
    expect(aliased["private-server/pro.png"]).toBe("/private-server/pro-nobg.png");
    expect(aliased["private-server/pro-model.png"]).toBe("/private-server/pro-nobg.png");
    expect(aliased["private-server/hero.png"]).toBe("/private-server/max-nobg.png");
    expect(aliased["private-server/servaultmax.jpg"]).toBe("/private-server/max-with-bg.jpg");
    expect(aliased["theron/hero.png"]).toBe("/theron/end-to-end-monitoring-system-ai-cam-ai-server.png");
    expect(aliased["theron/ai-chat.png"]).toBe("/theron/theron-chat.png");
    expect(aliased["theron/map-graph.png"]).toBe("/theron/theron-entity-graph.png");
    expect(aliased["theron/army-pilot.png"]).toBe("/theron/openwebui-rag-chat-army-data-darkmode.png");
    expect(aliased["theron/Chat-docs-lightmode.png"]).toBe("/theron/openwebui-rag-chat-army-data-lightmode.png");
    expect(aliased["theron/tank-detection.jpeg"]).toBe("/theron/ai-vision-enemy-tank-humans-detection.png");
    expect(aliased["theron/new_enemy_detection.png"]).toBe("/theron/ai-vision-enemy-tank-humans-detection.png");
    expect(aliased["theron/military-fusion-board.png"]).toBe("/theron/end-to-end-monitoring-system-ai-cam-ai-server.png");
    expect(aliased["wincommander/hero.png"]).toBe("/wincommander/wc-dashboard-lightmode.png");
    expect(aliased["wincommander/dashboard.png"]).toBe("/wincommander/wc-dashboard.png");
    expect(aliased["wincommander/forensic-trace-removal.png"]).toBe("/wincommander/wc-forensic-cleanup.png");
    expect(aliased["wincommander/privacy-settings.png"]).toBe("/wincommander/wc-privacy-settings.png");
    expect(aliased["wincommander/wincmd-dashboard.png"]).toBe("/wincommander/wc-dashboard-with-callouts.png");
    expect(aliased["wincommander/wincmd-network-control.png"]).toBe("/wincommander/wc-network-control-honeypot-callout.png");
    expect(aliased["contingency/decoy-hub.png"]).toBe("/contingency/usb-decoy-hub.png");
  });

  test("does not inject undefined aliases when no fallback asset exists", () => {
    const aliased = applyProductAliases({});
    const keys = Object.keys(aliased);

    expect(keys).not.toContain("contingency/decoy-hub.png");
    expect(keys).not.toContain("private-phone/hero.png");
    expect(keys).not.toContain("wincommander/hero.png");
  });
});
