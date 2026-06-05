// Dark-mode toggle, shared by every page that renders the site header.
//
// The initial theme is set by an inline <head> script (to avoid a flash of the
// wrong palette); here we just flip it on click and remember the choice. Safe to
// load on pages without a toggle button — it no-ops.

(function () {
  const themeToggle = document.getElementById("theme-toggle");
  if (!themeToggle) return;

  const themeColor = document.querySelector('meta[name="theme-color"]');
  const isDark = () =>
    document.documentElement.getAttribute("data-theme") === "dark";

  themeToggle.setAttribute("aria-pressed", String(isDark()));
  themeToggle.addEventListener("click", () => {
    const next = isDark() ? "light" : "dark";
    if (next === "dark") {
      document.documentElement.setAttribute("data-theme", "dark");
    } else {
      document.documentElement.removeAttribute("data-theme");
    }
    try {
      localStorage.setItem("theme", next);
    } catch (e) {}
    if (themeColor) {
      themeColor.setAttribute(
        "content",
        next === "dark" ? "#0f1419" : "#f4f6f9",
      );
    }
    themeToggle.setAttribute("aria-pressed", String(next === "dark"));
  });
})();
