// Applies the saved theme before the first paint. This is a file rather than an inline script
// because the CSP forbids inline scripts (docs/features/security.md). The key and values must
// match src/lib/theme.ts.
try {
  const theme = localStorage.getItem("uniwrt.theme");
  if (theme === "light" || theme === "dark") {
    document.documentElement.dataset.theme = theme;
  }
} catch {
  // Storage can be unavailable (blocked site data); the page then follows the system setting.
}
