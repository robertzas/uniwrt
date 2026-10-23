import tailwindcss from "@tailwindcss/vite";
import solid from "vite-plugin-solid";
import { defineConfig } from "vitest/config";

/** The backend the dev server proxies `/api` to: `uniwrt serve` on its default port. */
const api = process.env.UNIWRT_DEV_API ?? "http://127.0.0.1:8080";

export default defineConfig({
  plugins: [solid(), tailwindcss()],
  server: {
    proxy: { "/api": { target: api, ws: true } },
  },
  build: {
    // The Rust server caches `assets/` forever and answers a miss there with 404, never the SPA
    // page (docs/dev/BUILD.md, "Embedded UI"), so hashed files must stay in this directory.
    assetsDir: "assets",
    // The CSP allows fonts only from 'self' (security.md), so small fonts must not become data: URIs.
    assetsInlineLimit: (file) => (/\.(woff2?|ttf|otf)$/.test(file) ? false : undefined),
    // Every supported browser preloads modules natively.
    modulePreload: { polyfill: false },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    restoreMocks: true,
    unstubGlobals: true,
  },
});
