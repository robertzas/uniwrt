// Applies the saved theme and skin before the first paint. This is a file rather than an inline
// script because the CSP forbids inline scripts (docs/features/security.md). The keys and values
// must match src/lib/appearance.ts.
try {
  const root = document.documentElement;
  const theme = localStorage.getItem("uniwrt.theme");
  if (theme === "light" || theme === "dark") {
    root.dataset.theme = theme;
  }
  const skin = localStorage.getItem("uniwrt.skin");
  if (skin === "glass" || skin === "unifi") {
    root.dataset.skin = skin;
  }
} catch {
  // Storage can be unavailable (blocked site data); the page then uses the defaults.
}
