import "@testing-library/jest-dom/vitest";

import { cleanup, configure } from "@solidjs/testing-library";
import { afterEach } from "vitest";

afterEach(cleanup);

// On a busy CI runner a lazy route can take over a second to load: `find*` and `waitFor` wait up
// to 3 s (passing tests don't wait longer), and a test up to 15 s (vite.config.ts).
configure({ asyncUtilTimeout: 3_000 });

// jsdom has no layout, so it lacks ResizeObserver; Kobalte measures with it (indicators, popovers).
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

// Nor does it scroll; Kobalte scrolls the focused option of lists and menus into view.
Element.prototype.scrollIntoView ??= () => {};
