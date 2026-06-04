// Client-side filtering for the Rust Tool Index.
//
// Everything is already in the DOM, so filtering is pure show/hide: no network
// round-trips, and Ctrl-F keeps working. We filter tool cards by a precomputed
// `data-keywords` haystack, hide categories that end up empty, and optionally
// hide deprecated tools.

(function () {
  const input = document.getElementById("filter");
  const hideDeprecated = document.getElementById("hide-deprecated");
  const recommendedOnly = document.getElementById("recommended-only");
  const licenseFilter = document.getElementById("license-filter");
  const noResults = document.getElementById("no-results");
  const categories = Array.from(document.querySelectorAll(".category"));
  const tools = Array.from(document.querySelectorAll(".tool"));

  function apply() {
    const query = input.value.trim().toLowerCase();
    const terms = query.split(/\s+/).filter(Boolean);
    const skipDeprecated = hideDeprecated.checked;
    const onlyRecommended = recommendedOnly.checked;
    const license = licenseFilter.value;
    let visible = 0;

    for (const tool of tools) {
      const haystack = tool.dataset.keywords || "";
      const deprecated = tool.dataset.archived === "true";
      const recommended = tool.dataset.recommended === "true";
      const licenses = (tool.dataset.license || "")
        .split(/\s+/)
        .filter(Boolean);
      const matches =
        (!skipDeprecated || !deprecated) &&
        (!onlyRecommended || recommended) &&
        (!license || licenses.includes(license)) &&
        terms.every((t) => haystack.includes(t));
      tool.hidden = !matches;
      if (matches) visible++;
    }

    // Hide category sections (and their nav chips) that have no visible tools.
    for (const cat of categories) {
      const any = cat.querySelector(".tool:not([hidden])");
      cat.hidden = !any;
      const chip = document.querySelector(
        `.category-chip[data-cat="${cat.dataset.cat}"]`,
      );
      if (chip) chip.hidden = !any;
    }

    noResults.hidden = visible !== 0;
  }

  input.addEventListener("input", apply);
  hideDeprecated.addEventListener("change", apply);
  recommendedOnly.addEventListener("change", apply);
  licenseFilter.addEventListener("change", apply);

  // Allow `/` to focus the filter from anywhere.
  document.addEventListener("keydown", (e) => {
    if (e.key === "/" && document.activeElement !== input) {
      e.preventDefault();
      input.focus();
    }
  });
})();
