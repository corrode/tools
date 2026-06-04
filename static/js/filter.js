// Client-side filtering for the Rust Tool Index.
//
// Everything is already in the DOM, so filtering is pure show/hide: no network
// round-trips, and Ctrl-F keeps working. We filter tool cards by a precomputed
// `data-keywords` haystack, hide categories that end up empty, and optionally
// hide deprecated tools.

(function () {
  const input = document.getElementById("filter");
  const hideDeprecated = document.getElementById("hide-deprecated");
  const countEl = document.getElementById("filter-count");
  const noResults = document.getElementById("no-results");
  const categories = Array.from(document.querySelectorAll(".category"));
  const tools = Array.from(document.querySelectorAll(".tool"));

  function apply() {
    const query = input.value.trim().toLowerCase();
    const terms = query.split(/\s+/).filter(Boolean);
    const skipDeprecated = hideDeprecated.checked;
    let visible = 0;

    for (const tool of tools) {
      const haystack = tool.dataset.keywords || "";
      const deprecated = tool.dataset.archived === "true";
      const matches =
        (!skipDeprecated || !deprecated) &&
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
    countEl.textContent = query || skipDeprecated ? `${visible} shown` : "";
  }

  input.addEventListener("input", apply);
  hideDeprecated.addEventListener("change", apply);

  // Allow `/` to focus the filter from anywhere.
  document.addEventListener("keydown", (e) => {
    if (e.key === "/" && document.activeElement !== input) {
      e.preventDefault();
      input.focus();
    }
  });
})();
