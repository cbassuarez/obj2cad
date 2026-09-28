import { defineConfig } from "vite";
import { VitePWA } from "vite-plugin-pwa";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath } from "node:url";
import { readFileSync } from "node:fs";

const pkg = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8"));

// BASE is set by the deploy workflow ("/obj2cad/" on GitHub Pages, "/obj2cad/v/x.y.z/" for
// the pinned copy of each release). Pinned copies are archival: no service worker, so they
// never update themselves and never interfere with the latest version's worker.
const pinned = process.env.PINNED === "1";

export default defineConfig({
  base: process.env.BASE ?? "/",
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
  worker: { format: "es" },
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  build: { target: "es2022", sourcemap: true, chunkSizeWarningLimit: 800 },
  plugins: [
    react(),
    tailwindcss(),
    VitePWA({
      disable: pinned,
      registerType: "prompt",
      injectRegister: false,
      includeAssets: ["icon.svg"],
      manifest: {
        name: "obj2cad",
        short_name: "obj2cad",
        description: "Exact OBJ to DWG/DXF conversion, entirely on your computer.",
        theme_color: "#0e0f11",
        background_color: "#0e0f11",
        display: "standalone",
        icons: [{ src: "icon.svg", sizes: "any", type: "image/svg+xml", purpose: "any" }],
      },
      workbox: {
        globPatterns: ["**/*.{js,css,html,svg,wasm,woff2}"],
        maximumFileSizeToCacheInBytes: 8 * 1024 * 1024,
        // Pinned releases live under v/<version>/ and must load their own index.html.
        navigateFallbackDenylist: [/\/v\/\d/],
      },
    }),
  ],
});
