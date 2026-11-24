/** What stands in for xterm's `<style>` elements while it opens: inert, hidden, never applied. */
const INERT = "uniwrt-style";

/**
 * Runs `open` (xterm's `open()`) with each `<style>` element it makes swapped for an inert one,
 * so that the CSP has nothing to refuse (and logs no error); [`mirrorStyles`] applies their CSS.
 * Only what `open` makes at once is swapped: a renderer made later (after a lost WebGL context)
 * makes real ones, which the CSP refuses and the mirror applies all the same.
 */
export function withInertStyles<T>(open: () => T): T {
  const own = Object.getOwnPropertyDescriptor(document, "createElement");
  const create = document.createElement;
  document.createElement = function (
    this: Document,
    tag: string,
    options?: ElementCreationOptions,
  ) {
    if (tag.toLowerCase() !== "style") return create.call(this, tag, options);
    const inert = create.call(this, INERT);
    inert.hidden = true;
    return inert;
  } as typeof document.createElement;
  try {
    return open();
  } finally {
    if (own) Object.defineProperty(document, "createElement", own);
    else delete (document as { createElement?: unknown }).createElement;
  }
}

/**
 * xterm.js writes some of its CSS into `<style>` elements (its scrollbar's colors; the DOM
 * renderer's, when WebGL is missing), which the CSP blocks (`default-src 'self'`, no inline
 * styles). The CSP allows the CSSOM, so this copies their text into one constructed stylesheet
 * the document adopts, and keeps it in step as xterm rewrites them. Returns the undo.
 */
export function mirrorStyles(root: HTMLElement): () => void {
  if (typeof CSSStyleSheet === "undefined" || !("adoptedStyleSheets" in document)) {
    return () => undefined;
  }
  const sheet = new CSSStyleSheet();
  document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
  let last = "";
  const sync = () => {
    const text = Array.from(
      root.querySelectorAll(`style, ${INERT}`),
      (style) => style.textContent,
    ).join("\n");
    if (text === last) return;
    last = text;
    sheet.replaceSync(text);
  };
  const observer = new MutationObserver(sync);
  observer.observe(root, { childList: true, subtree: true, characterData: true });
  sync();
  return () => {
    observer.disconnect();
    document.adoptedStyleSheets = document.adoptedStyleSheets.filter((other) => other !== sheet);
  };
}
