import { defineConfig, searchForWorkspaceRoot, type UserConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "path";

const tauriDevHost = process.env.TAURI_DEV_HOST;
const host = tauriDevHost || "localhost";
// Runtime artwork is supplied by the pinned `assets` submodule. Production
// builds and local development deliberately use the same source tree.
const bundledAssetsRoot = path.resolve(__dirname, "../assets");
const localAssetsRoot = path.resolve(__dirname, "./assets");

export default defineConfig(({ command }): UserConfig => ({
  plugins: [...react(), ...tailwindcss()],
  resolve: {
    alias: [
      { find: "@", replacement: path.resolve(__dirname, "./src") },
      // `@assets` also exposes the local React RiskMatrix component, which
      // resolves its own React dependency through this app's node_modules.
      { find: "@assets", replacement: localAssetsRoot },
      // Vite emits the external glob imports under `/assets/...` while
      // developing. Resolve that URL-shaped module id back to the shared
      // workspace tree so its `?url` module can be transformed normally.
      { find: /^\/assets\//, replacement: `${bundledAssetsRoot}/` },
    ],
  },
  // KT: Disable source maps in production to prevent frontend code recovery.
  // Vite still generates them in dev for debugging.
  build: {
    sourcemap: false,
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
        localAssetsRoot,
        bundledAssetsRoot,
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
