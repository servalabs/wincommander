import { defineConfig, searchForWorkspaceRoot, type Plugin, type UserConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import fs from "node:fs";
import path from "path";

// Tauri injects TAURI_DEV_HOST=localhost for ordinary desktop development.
// Do not let that hostname choose the bind address: WebView2 can resolve
// localhost differently between the document and a later module fetch. Remote
// device development still supplies a non-loopback host and keeps its HMR
// configuration below.
const requestedTauriDevHost = process.env.TAURI_DEV_HOST;
const tauriDevHost = requestedTauriDevHost
  && !["localhost", "127.0.0.1", "::1"].includes(requestedTauriDevHost)
  ? requestedTauriDevHost
  : undefined;
const host = tauriDevHost || "127.0.0.1";
// Runtime artwork imported as `/assets/...` is supplied by WinCommander's
// pinned `assets` submodule (`./assets`). Product media globs in
// `src/assets.ts` also resolve against this same root (`../assets/...` from
// `src/`). The optional workspace sibling `../assets` is only allowed for
// rare `/@fs/...` paths — do not point product globs at it.
const appAssetsRoot = path.resolve(__dirname, "./assets");
const workspaceAssetsRoot = path.resolve(__dirname, "../assets");

const assetContentTypes: Record<string, string> = {
  ".avif": "image/avif",
  ".gif": "image/gif",
  ".ico": "image/x-icon",
  ".jpeg": "image/jpeg",
  ".jpg": "image/jpeg",
  ".mp4": "video/mp4",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".webp": "image/webp",
  ".webm": "video/webm",
};

/** Serves WinCommander's imported `/assets` media without shadowing Vite modules. */
function serveAppAssetsInDevelopment(): Plugin {
  return {
    name: "wincommander-app-assets",
    apply: "serve",
    configureServer(server) {
      server.middlewares.use("/assets", (request, response, next) => {
        const requestUrl = new URL(request.url ?? "/", "http://vite.local");
        // Preserve Vite's JavaScript `?url` module transform. This middleware
        // handles only the final image/media URL returned by that module.
        if (requestUrl.searchParams.has("import")) {
          next();
          return;
        }

        const requestedPath = decodeURIComponent(requestUrl.pathname);
        const filePath = path.resolve(appAssetsRoot, `.${requestedPath}`);
        const relativePath = path.relative(appAssetsRoot, filePath);
        if (relativePath.startsWith("..") || path.isAbsolute(relativePath)) {
          next();
          return;
        }

        // `/assets` also contains source modules such as
        // components/risk-matrix/index.ts. Only serve known binary media here;
        // Vite must transform TypeScript/CSS/JSON module requests itself.
        // Serving an unknown extension as application/octet-stream makes
        // WebView2 reject it as a module script and leaves the app blank.
        const contentType = assetContentTypes[path.extname(filePath).toLowerCase()];
        if (!contentType) {
          next();
          return;
        }

        fs.stat(filePath, (error, stats) => {
          if (error || !stats.isFile()) {
            next();
            return;
          }

          response.statusCode = 200;
          response.setHeader("Content-Type", contentType);
          response.setHeader("Cache-Control", "no-store");
          if (request.method === "HEAD") {
            response.end();
            return;
          }
          fs.createReadStream(filePath).pipe(response);
        });
      });
    },
  };
}

/** Emits a reviewable snapshot of the main entry's static dependency graph. */
function reportInitialResources(): Plugin {
  return {
    name: "wincommander-initial-resource-report",
    apply: "build",
    generateBundle(_, bundle) {
      // Vite's HTML entry owns the root chunk; `src/main.tsx` is a module of
      // that chunk rather than its facade. There is only one browser entry in
      // this desktop build, so the first entry chunk is the initial resource.
      const entry = Object.values(bundle).find(
        (item) => item.type === "chunk" && item.isEntry,
      );
      if (!entry || entry.type !== "chunk") return;
      const mainWindow = Object.values(bundle).find(
        (item) => item.type === "chunk" && /(?:^|[\\/])entries[\\/]mainWindow\.tsx$/.test(item.facadeModuleId ?? ""),
      );

      const initialChunks = new Set<string>();
      const visit = (fileName: string) => {
        if (initialChunks.has(fileName)) return;
        initialChunks.add(fileName);
        const chunk = bundle[fileName];
        if (chunk?.type === "chunk") chunk.imports.forEach(visit);
      };
      visit(entry.fileName);
      // The main window root is selected immediately for the normal desktop
      // label, unlike the search and notification auxiliary roots.
      if (mainWindow?.type === "chunk") visit(mainWindow.fileName);

      const files = [...initialChunks]
        .map((fileName) => bundle[fileName])
        .filter((item): item is Extract<(typeof bundle)[string], { type: "chunk" }> => item?.type === "chunk")
        .map((item) => ({ file: item.fileName, bytes: Buffer.byteLength(item.code) }));
      const fonts = Object.values(bundle)
        .filter((item) => item.type === "asset" && /\.woff2$/i.test(item.fileName))
        .map((item) => ({ file: item.fileName, bytes: typeof item.source === "string" ? Buffer.byteLength(item.source) : item.source.byteLength }));
      const report = {
        entry: entry.fileName,
        mainWindow: mainWindow?.type === "chunk" ? mainWindow.fileName : null,
        initialJavaScriptBytes: files.reduce((total, item) => total + item.bytes, 0),
        defaultFontBytes: fonts.reduce((total, item) => total + item.bytes, 0),
        initialChunks: files,
        fonts,
      };
      this.emitFile({
        type: "asset",
        fileName: "performance-initial-resources.json",
        source: `${JSON.stringify(report, null, 2)}\n`,
      });
    },
  };
}

export default defineConfig(({ command }): UserConfig => ({
  plugins: [serveAppAssetsInDevelopment(), reportInitialResources(), ...react(), ...tailwindcss()],
  resolve: {
    alias: [
      { find: "@", replacement: path.resolve(__dirname, "./src") },
      // `@assets` also exposes the local React RiskMatrix component, which
      // resolves its own React dependency through this app's node_modules.
      { find: "@assets", replacement: appAssetsRoot },
    ],
  },
  // KT: Disable source maps in production to prevent frontend code recovery.
  // Vite still generates them in dev for debugging.
  build: {
    sourcemap: false,
    // Never inline media as base64 (Vite's default 4KB threshold would). The
    // release pipeline's "Verify bundled shared media" step (release.yml)
    // greps dist/assets for physical, hashed filenames per pinned asset — an
    // asset small enough to fall under that threshold (e.g. a compressed
    // logo) silently becomes a data: URI instead of a file, and that check
    // fails even though the build itself is fine.
    assetsInlineLimit: 0,
    // esbuild is 10-20x faster than terser and handles console/debugger
    // drops natively. Terser's property mangling was disabled anyway
    // (`properties: false`), so there is no obfuscation regression.
    minify: "esbuild" as const,
    // Manual chunking — split the heavy dependencies into named
    // chunks so the Flows panel doesn't pull @xyflow into the
    // initial bundle, and so Blueprint / framer-motion / react-query
    // can be cached independently of app code. Panels are already
    // lazy-loaded via PANEL_MANIFESTS, but their shared vendor code
    // ends up in the entry chunk without these splits.
    rollupOptions: {
      output: {
        manualChunks: (id: string) => {
          if (!id.includes("node_modules")) return undefined;
          if (id.includes("@xyflow")) return "vendor-flows";
          if (id.includes("@radix-ui") || id.includes("cmdk")) return "vendor-radix";
          if (id.includes("framer-motion")) return "vendor-motion";
          if (id.includes("@tanstack/react-query")) return "vendor-query";
          if (id.includes("@tauri-apps")) return "vendor-tauri";
          if (id.includes("react-dom") || id.includes("/react/")) return "vendor-react";
          return undefined;
        },
      },
    },
    chunkSizeWarningLimit: 600,
  },
  esbuild: {
    // Drop console.* and debugger statements in production builds only.
    drop: command === 'build' ? ["console", "debugger"] : [],
  },
  clearScreen: false,
  server: {
    // WebView2 persists its HTTP cache across dev-app restarts. Never retain
    // transformed module responses, or a raw asset cached under a Vite import
    // URL can prevent React from mounting and leave an undiagnosed white page.
    headers: {
      "Cache-Control": "no-store",
    },
    fs: {
      allow: [
        searchForWorkspaceRoot(process.cwd()),
        appAssetsRoot,
        workspaceAssetsRoot,
      ],
    },
    port: 1420,
    strictPort: true,
    host,
    // Let Vite derive the local HMR socket from the page URL. The previous
    // "::1" default produced the invalid URL `ws://::1:1421`, leaving the
    // WebView without Vite's CSS runtime and crashing every lazy panel that
    // imported styles. TAURI_DEV_HOST is only set for remote-device dev.
    hmr: tauriDevHost
      ? {
          protocol: "ws",
          host: tauriDevHost,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: [
        "**/src-tauri/**",
        "**/docs/**",
        "**/Readme.md",
        "**/dist/**",
        "**/README.md",
        "**/.git/**",
      ],
    },
  },
}));
