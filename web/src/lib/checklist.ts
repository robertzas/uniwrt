import { createSignal } from "solid-js";

/**
 * Whether this browser hid the dashboard's *Get started* checklist (D-148), shared by the
 * dashboard, the command palette and the help center (D-151, M9.5d), which can show it again.
 * Reactive: a change elsewhere shows or hides the checklist on an open dashboard.
 */
const KEY = "uniwrt.getStarted.dismissed";
const [version, bump] = createSignal(0);

/** Whether this browser was told to hide the checklist. */
export function dismissed(): boolean {
  version();
  try {
    return localStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

/** Hides the checklist in this browser, or shows it again. */
export function setDismissed(value: boolean): void {
  try {
    if (value) localStorage.setItem(KEY, "1");
    else localStorage.removeItem(KEY);
  } catch {
    // Unavailable storage only means the choice is not remembered.
  }
  bump((n) => n + 1);
}
