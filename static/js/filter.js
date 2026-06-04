// Client-side filtering, sorting, and shareable state for the Rust Tool Index.
//
// Everything is already in the DOM, so filtering is pure show/hide: no network
// round-trips, and Ctrl-F keeps working. We filter tool rows by a precomputed
// `data-keywords` haystack, hide categories that end up empty, optionally hide
// deprecated or non-recommended tools, sort within each category, and mirror
// the whole state into the URL query string so a view is linkable.

(function () {
  const input = document.getElementById("filter");
  const hideDeprecated = document.getElementById("hide-deprecated");
  const recommendedOnly = document.getElementById("recommended-only");
  const licenseFilter = document.getElementById("license-filter");
  const sortSelect = document.getElementById("sort-select");
  const stackFilter = document.getElementById("stack-filter");
  const stackBanners = Array.from(document.querySelectorAll(".stack-banner"));
  const stackNotes = Array.from(document.querySelectorAll(".stack-note"));
  const noResults = document.getElementById("no-results");
  const categories = Array.from(document.querySelectorAll(".category"));
  const tools = Array.from(document.querySelectorAll(".tool"));

  // Remember each tool's original (server-ranked) position so the "Relevance"
  // sort can always restore it.
  tools.forEach((tool, i) => {
    tool.dataset.order = String(i);
  });

  const num = (el, key) => Number(el.dataset[key] || 0);
  const str = (el, key) => el.dataset[key] || "";

  // Comparators return rows in display order. All but `name`/`default` are
  // descending; rows missing a value sort last.
  const comparators = {
    default: (a, b) => num(a, "order") - num(b, "order"),
    downloads: (a, b) => num(b, "downloads") - num(a, "downloads"),
    stars: (a, b) => num(b, "stars") - num(a, "stars"),
    updated: (a, b) => str(b, "updated").localeCompare(str(a, "updated")),
    added: (a, b) => str(b, "added").localeCompare(str(a, "added")),
    name: (a, b) =>
      str(a, "name").localeCompare(str(b, "name"), undefined, {
        sensitivity: "base",
      }),
  };

  function sortRows(mode) {
    const cmp = comparators[mode] || comparators.default;
    for (const cat of categories) {
      const body = cat.querySelector("tbody");
      if (!body) continue;
      const rows = Array.from(body.querySelectorAll(".tool"));
      rows.sort(cmp);
      for (const row of rows) body.appendChild(row);
    }
  }

  function apply() {
    const query = input.value.trim().toLowerCase();
    const terms = query.split(/\s+/).filter(Boolean);
    const skipDeprecated = hideDeprecated.checked;
    const onlyRecommended = recommendedOnly.checked;
    const license = licenseFilter.value;
    const stack = stackFilter ? stackFilter.value : "";
    let visible = 0;

    for (const tool of tools) {
      const haystack = tool.dataset.keywords || "";
      const deprecated = tool.dataset.archived === "true";
      const recommended = tool.dataset.recommended === "true";
      const licenses = (tool.dataset.license || "")
        .split(/\s+/)
        .filter(Boolean);
      const stacks = (tool.dataset.stacks || "").split(/\s+/).filter(Boolean);
      const matches =
        (!skipDeprecated || !deprecated) &&
        (!onlyRecommended || recommended) &&
        (!license || licenses.includes(license)) &&
        (!stack || stacks.includes(stack)) &&
        terms.every((t) => haystack.includes(t));
      tool.hidden = !matches;
      if (matches) visible++;
    }

    sortRows(sortSelect.value);

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
    updateStackContext(stack);
    syncUrl();
  }

  // Reveal the selected stack's banner and its picks' inline notes; everything
  // for other stacks (and all of it, when no stack is active) stays hidden.
  function updateStackContext(active) {
    for (const banner of stackBanners) {
      banner.hidden = banner.dataset.stack !== active;
    }
    for (const note of stackNotes) {
      note.hidden = note.dataset.stack !== active;
    }
  }

  // ── Shareable URL state ──────────────────────────────────────────────────
  // Only non-default values are written, so a pristine page stays at `/`.

  function syncUrl() {
    const params = new URLSearchParams();
    if (input.value.trim()) params.set("q", input.value.trim());
    if (licenseFilter.value) params.set("license", licenseFilter.value);
    if (sortSelect.value !== "default") params.set("sort", sortSelect.value);
    if (hideDeprecated.checked) params.set("deprecated", "0");
    if (recommendedOnly.checked) params.set("recommended", "1");
    if (stackFilter && stackFilter.value)
      params.set("stack", stackFilter.value);
    const qs = params.toString();
    const url = qs ? `?${qs}` : location.pathname;
    history.replaceState(null, "", url + location.hash);
  }

  function restoreFromUrl() {
    const params = new URLSearchParams(location.search);
    if (params.has("q")) input.value = params.get("q");
    const license = params.get("license");
    if (
      license &&
      [...licenseFilter.options].some((o) => o.value === license)
    ) {
      licenseFilter.value = license;
    }
    const sort = params.get("sort");
    if (sort && comparators[sort]) sortSelect.value = sort;
    hideDeprecated.checked = params.get("deprecated") === "0";
    recommendedOnly.checked = params.get("recommended") === "1";
    const stack = params.get("stack");
    if (
      stackFilter &&
      stack &&
      [...stackFilter.options].some((o) => o.value === stack)
    ) {
      stackFilter.value = stack;
    }
  }

  restoreFromUrl();
  apply();

  input.addEventListener("input", apply);
  hideDeprecated.addEventListener("change", apply);
  recommendedOnly.addEventListener("change", apply);
  licenseFilter.addEventListener("change", apply);
  sortSelect.addEventListener("change", apply);
  if (stackFilter) stackFilter.addEventListener("change", apply);

  // The "In <stack>" chips and a banner's "Clear filter" button drive the same
  // dropdown, filtering in place instead of navigating away.
  document.addEventListener("click", (e) => {
    const chip = e.target.closest(".stack-chip[data-stack]");
    if (chip && stackFilter) {
      e.preventDefault();
      stackFilter.value = chip.dataset.stack;
      apply();
      document
        .getElementById("catalog")
        .scrollIntoView({ behavior: "smooth", block: "start" });
    } else if (e.target.closest("[data-clear-stack]") && stackFilter) {
      e.preventDefault();
      stackFilter.value = "";
      apply();
    }
  });

  // Allow `/` to focus the filter from anywhere.
  document.addEventListener("keydown", (e) => {
    if (e.key === "/" && document.activeElement !== input) {
      e.preventDefault();
      input.focus();
    }
  });
})();
