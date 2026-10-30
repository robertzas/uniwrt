import "@testing-library/jest-dom/vitest";

import { cleanup } from "@solidjs/testing-library";
import { afterEach } from "vitest";

afterEach(cleanup);

// jsdom has no layout, so it lacks ResizeObserver; Kobalte measures with it (indicators, popovers).
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

// Nor does it scroll; Kobalte scrolls the focused option of lists and menus into view.
Element.prototype.scrollIntoView ??= () => {};
