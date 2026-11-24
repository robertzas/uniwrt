/**
 * xterm.js, loaded with the Terminal tab only (it and its WebGL renderer are ~460 KB). The WebGL
 * renderer is loaded before `open()`, so xterm never makes its DOM renderer, whose CSS goes in
 * `<style>` elements and `style` attributes the CSP blocks; where WebGL2 is missing (or its
 * context is lost) xterm falls back to that renderer, and [`mirrorStyles`] carries its CSS.
 */
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { type ITheme, Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { mirrorStyles, withInertStyles } from "./styles";

/** The terminal as the session uses it. */
export interface XtermHandle {
  write: (data: Uint8Array | string) => void;
  /** Keystrokes, pastes and terminal reports, as bytes for the shell. */
  onInput: (handler: (bytes: Uint8Array<ArrayBuffer>) => void) => void;
  onResize: (handler: (cols: number, rows: number) => void) => void;
  /** Fits the terminal to its element, and answers its size. */
  fit: () => { cols: number; rows: number };
  focus: () => void;
  dispose: () => void;
}

/** The terminal's background, also the frame's (`bg-[#0d1117]` in TerminalTab). */
export const BACKGROUND = "#0d1117";

const THEME: ITheme = {
  background: BACKGROUND,
  foreground: "#e6edf3",
  cursor: "#58a6ff",
  cursorAccent: BACKGROUND,
  selectionBackground: "#264f78",
  scrollbarSliderBackground: "rgba(110, 118, 129, 0.35)",
  scrollbarSliderHoverBackground: "rgba(110, 118, 129, 0.55)",
  scrollbarSliderActiveBackground: "rgba(110, 118, 129, 0.7)",
  black: "#484f58",
  red: "#ff7b72",
  green: "#3fb950",
  yellow: "#d29922",
  blue: "#58a6ff",
  magenta: "#bc8cff",
  cyan: "#39c5cf",
  white: "#b1bac4",
  brightBlack: "#6e7681",
  brightRed: "#ffa198",
  brightGreen: "#56d364",
  brightYellow: "#e3b341",
  brightBlue: "#79c0ff",
  brightMagenta: "#d2a8ff",
  brightCyan: "#56d4dd",
  brightWhite: "#ffffff",
};

const FONT = '"JetBrains Mono Variable", ui-monospace, monospace';
const SIZE = 13;

/**
 * Whether the browser gives WebGL2 contexts. The addon would throw where it doesn't, and xterm
 * reports that as an uncaught error before falling back to its DOM renderer.
 */
function webgl2(): boolean {
  const context = document.createElement("canvas").getContext("webgl2");
  context?.getExtension("WEBGL_lose_context")?.loseContext();
  return context !== null;
}

/** Opens a terminal in `host`, which must be in the document. */
export async function createXterm(host: HTMLElement): Promise<XtermHandle> {
  // xterm measures its cells once it opens: with a fallback font they would be off.
  await document.fonts?.load(`${SIZE}px "JetBrains Mono Variable"`).catch(() => undefined);
  const terminal = new Terminal({
    fontFamily: FONT,
    fontSize: SIZE,
    lineHeight: 1.15,
    cursorBlink: true,
    scrollback: 5_000,
    theme: THEME,
  });
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  if (webgl2()) {
    const webgl = new WebglAddon();
    webgl.onContextLoss(() => webgl.dispose());
    terminal.loadAddon(webgl);
  }
  const unmirror = mirrorStyles(host);
  withInertStyles(() => terminal.open(host));
  const encoder = new TextEncoder();
  return {
    write: (data) => terminal.write(data),
    onInput: (handler) => {
      terminal.onData((data) => handler(encoder.encode(data)));
      // Some mouse reports are bytes, not UTF-8.
      terminal.onBinary((data) => handler(Uint8Array.from(data, (c) => c.charCodeAt(0) & 0xff)));
    },
    onResize: (handler) => {
      terminal.onResize(({ cols, rows }) => handler(cols, rows));
    },
    fit: () => {
      if (fit.proposeDimensions()) fit.fit();
      return { cols: terminal.cols, rows: terminal.rows };
    },
    focus: () => terminal.focus(),
    dispose: () => {
      terminal.dispose();
      unmirror();
    },
  };
}
