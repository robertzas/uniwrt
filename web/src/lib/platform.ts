/** True on Apple platforms, where shortcuts use ⌘ instead of Ctrl. */
export const isApple = /Mac|iPhone|iPad/.test(globalThis.navigator?.userAgent ?? "");

/** The palette's shortcut as the platform writes it. */
export const paletteShortcut = isApple ? "⌘K" : "Ctrl K";

/** Whether a keydown is the palette's shortcut: ⌘K on Apple platforms, Ctrl+K elsewhere. */
export function isPaletteShortcut(event: KeyboardEvent): boolean {
  return (
    event.key.toLowerCase() === "k" && (isApple ? event.metaKey : event.ctrlKey) && !event.altKey
  );
}
