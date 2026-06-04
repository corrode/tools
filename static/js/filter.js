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
  const stackCards = Array.from(
    document.querySelectorAll(".stack-card[data-stack]"),
  );
  const categoryJump = document.getElementById("category-jump");
  const noResults = document.getElementById("no-results");
  const categories = Array.from(document.querySelectorAll(".category"));
  const tools = Array.from(document.querySelectorAll(".tool"));
  const collapseAllBtn = document.getElementById("collapse-all");

  // ── Collapsible categories ──────────────────────────────────────
  // Manual collapse is a navigation aid, persisted per-browser. It's a layer on
  // top of filtering, not part of it: an active search always wins, so matches
  // are never hidden inside a folded section.
  const COLLAPSE_KEY = "collapsedCategories";
  const collapsed = new Set(loadCollapsed());

  function loadCollapsed() {
    try {
      const raw = JSON.parse(localStorage.getItem(COLLAPSE_KEY) || "[]");
      return Array.isArray(raw) ? raw : [];
    } catch (e) {
      return [];
    }
  }

  function persistCollapsed() {
    try {
      localStorage.setItem(COLLAPSE_KEY, JSON.stringify([...collapsed]));
    } catch (e) {}
  }

  // Reflect the collapse set into the DOM. A non-empty query forces every
  // category open so results stay visible; the saved set returns when it clears.
  function renderCollapsed() {
    const searchActive = input.value.trim() !== "";
    for (const cat of categories) {
      const isCollapsed = !searchActive && collapsed.has(cat.dataset.cat);
      cat.classList.toggle("collapsed", isCollapsed);
      const toggle = cat.querySelector(".category-toggle");
      if (toggle) {
        toggle.setAttribute("aria-expanded", String(!isCollapsed));
      }
    }
    if (collapseAllBtn) {
      const shown = categories.filter((c) => !c.hidden);
      const allCollapsed =
        shown.length > 0 && shown.every((c) => collapsed.has(c.dataset.cat));
      collapseAllBtn.disabled = searchActive;
      collapseAllBtn.setAttribute("aria-pressed", String(allCollapsed));
      collapseAllBtn.textContent = allCollapsed ? "Expand all" : "Collapse all";
    }
  }

  function setCollapsed(id, value) {
    if (value) collapsed.add(id);
    else collapsed.delete(id);
    persistCollapsed();
    renderCollapsed();
  }

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

    // Hide category sections that end up empty, and disable their entry in the
    // "Jump to category" menu so it can't scroll to nothing.
    for (const cat of categories) {
      const any = cat.querySelector(".tool:not([hidden])");
      cat.hidden = !any;
      if (categoryJump) {
        const opt = categoryJump.querySelector(
          `option[value="cat-${cat.dataset.cat}"]`,
        );
        if (opt) opt.disabled = !any;
      }
    }

    noResults.hidden = visible !== 0;
    renderCollapsed();
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
    for (const card of stackCards) {
      card.setAttribute("aria-pressed", String(card.dataset.stack === active));
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

  // Click a category header (or its chevron) to fold it away. The chevron is a
  // real <button>, so keyboard users get the same toggle for free.
  for (const cat of categories) {
    const head = cat.querySelector(".category-head");
    if (!head) continue;
    head.addEventListener("click", (e) => {
      if (e.target.closest("a")) return;
      const sel = window.getSelection();
      if (sel && !sel.isCollapsed && head.contains(sel.anchorNode)) return;
      setCollapsed(cat.dataset.cat, !collapsed.has(cat.dataset.cat));
    });
  }

  // Fold or unfold every visible category at once — turns the page into a
  // scannable index of category headers.
  if (collapseAllBtn) {
    collapseAllBtn.addEventListener("click", () => {
      const shown = categories.filter((c) => !c.hidden);
      const allCollapsed = shown.every((c) => collapsed.has(c.dataset.cat));
      if (allCollapsed) {
        collapsed.clear();
      } else {
        for (const c of shown) collapsed.add(c.dataset.cat);
      }
      persistCollapsed();
      renderCollapsed();
    });
  }

  // Stack selection flows through the hidden <select> (the single source of
  // truth for the active stack, URL sync, and row filtering): the picker cards,
  // the "In <stack>" row chips, and a banner's "Clear filter" button all just
  // set its value and re-apply.
  document.addEventListener("click", (e) => {
    // Picker chips toggle the stack on/off in place. We deliberately do NOT
    // scroll: the chips sit at the top, the banner appears just below them, and
    // scrolling would jump the page down out from under the user.
    const card = e.target.closest(".stack-card[data-stack]");
    if (card && stackFilter) {
      e.preventDefault();
      const id = card.dataset.stack;
      stackFilter.value = stackFilter.value === id ? "" : id;
      apply();
      return;
    }
    const chip = e.target.closest(".chip[data-stack]");
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

  // "Jump to category" is pure navigation: scroll to the chosen section, then
  // reset so the control always reads "Category…".
  if (categoryJump) {
    categoryJump.addEventListener("change", () => {
      const id = categoryJump.value;
      const section = id && document.getElementById(id);
      if (section && !section.hidden) {
        // Unfold the target so the jump lands on content, not a folded header.
        setCollapsed(section.dataset.cat, false);
        section.scrollIntoView({ behavior: "smooth", block: "start" });
      }
      categoryJump.value = "";
    });
  }

  // Allow `/` to focus the filter from anywhere.
  document.addEventListener("keydown", (e) => {
    if (e.key === "/" && document.activeElement !== input) {
      e.preventDefault();
      input.focus();
    }
  });

  // Dark-mode toggle. The initial theme is set by an inline <head> script (to
  // avoid a flash); here we just flip it on click and remember the choice.
  const themeToggle = document.getElementById("theme-toggle");
  if (themeToggle) {
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
  }
})();
