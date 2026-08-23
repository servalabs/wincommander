import { describe, expect, test } from "bun:test";
import { applyProductAliases } from "./assets/products";

declare const Bun: {
  file(path: string): {
    text(): Promise<string>;
  };
};

describe("applyProductAliases", () => {
  test("keeps shared asset module URLs separate from raw WebView asset URLs", async () => {
    const [appsSource, networkSource, featureSource, productSource] = await Promise.all([
      Bun.file("src/assets/apps.ts").text(),
      Bun.file("src/assets/network.ts").text(),
      Bun.file("src/assets/featureLogos.ts").text(),
      Bun.file("src/assets/products.ts").text(),
    ]);
    const featureQueries = featureSource.match(/query: "\?url&wc-module"/g) ?? [];
    const productQueries = productSource.match(/query: "\?url&wc-module"/g) ?? [];

    expect(appsSource).toContain('query: "?url&wc-module"');
    expect(networkSource).toContain('query: "?url&wc-module"');
    expect(featureQueries).toHaveLength(2);
    expect(productQueries).toHaveLength(3);
    expect(appsSource).not.toContain('query: "?url"');
    expect(networkSource).not.toContain('query: "?url"');
    expect(productSource).not.toContain('query: "?url"');
    // Product media must load from the app's assets submodule, not a
    // missing workspace sibling at ../../assets (that left the title logo empty).
    expect(productSource).toContain('../../assets/products/wincommander/Scrub.gif');
    expect(appsSource).not.toContain('assets/products/');
    expect(networkSource).not.toContain('assets/products/');
    expect(productSource).not.toContain('../../assets/products/wincommander/**/*');
    expect(productSource).not.toContain('../../assets/products/private-server/**/*');
    expect(productSource).not.toContain('../../assets/products/private-phone/**/*');
    expect(productSource).not.toContain('../../assets/products/theron/**/*');
    expect(appsSource).not.toContain('../assets/softwares/**/*",');
    expect(featureSource).not.toContain('assets/softwares/**/*');
  });

  test("keeps large asset families behind their consuming feature modules", async () => {
    const [registrySource, appIconsSource, browserSource, meshSource] = await Promise.all([
      Bun.file("src/registry/features.ts").text(),
      Bun.file("src/panels/apps/components/appIcons.ts").text(),
      Bun.file("src/panels/privacy/BrowserHardeningSection.tsx").text(),
      Bun.file("src/panels/mesh/MeshGrid.tsx").text(),
    ]);

    expect(registrySource).toContain('from "@/assets/featureLogos"');
    expect(appIconsSource).toContain('from "@/assets/apps"');
    expect(browserSource).toContain('from "../../assets/privacy"');
    expect(meshSource).toContain("from '@/assets/mesh'");
  });

  test("exposes the bundled Contingency clips through the UI media map", async () => {
    const source = await Bun.file("src/assets/products.ts").text();

    expect(source).toMatch(/export const ui[\s\S]*privateServerMods/);
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
