// Client-side filtering, sorting, and shareable state for the Rust Tool Index.
//
// Everything is already in the DOM, so filtering is pure show/hide: no network
// round-trips, and Ctrl-F keeps working. We filter tool rows by a precomputed
// `data-keywords` haystack, hide categories that end up empty, optionally hide
// deprecated or non-recommended tools, sort within each category, and mirror
// the whole state into the URL query string so a view is linkable.

(function () {
  const input = document.getElementById("filter");
  const searchClear = document.getElementById("search-clear");
  const hideDeprecated = document.getElementById("hide-deprecated");
  const recommendedOnly = document.getElementById("recommended-only");
  const licenseFilter = document.getElementById("license-filter");
  const msrvFilter = document.getElementById("msrv-filter");
  const sortSelect = document.getElementById("sort-select");
  const stackFilter = document.getElementById("stack-filter");
  const stackBanners = Array.from(document.querySelectorAll(".stack-banner"));
  const stackNotes = Array.from(document.querySelectorAll(".stack-note"));
  const stackOptions = Array.from(
    document.querySelectorAll(".stack-option[data-stack]"),
  );
  const stackDropdown = document.getElementById("stack-dropdown");
  const stackStart = document.querySelector(".stack-start");
  const categoryJump = document.getElementById("category-jump");
  const noResults = document.getElementById("no-results");
  const categories = Array.from(document.querySelectorAll(".category"));
  const tools = Array.from(document.querySelectorAll(".tool"));
  const sectionToggleAllBtn = document.getElementById("section-toggle-all");
  const sectionToggleAllLabel = document.getElementById(
    "section-toggle-all-label",
  );
  const filtersToggle = document.getElementById("filters-toggle");
  const catalogControls = document.querySelector(".catalog-controls");
  const filtersCount = document.querySelector(".filters-count");
  const siteHeader = document.querySelector(".site-header");

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
    const shown = categories.filter((c) => !c.hidden);
    const anyCollapsed =
      !searchActive && shown.some((c) => collapsed.has(c.dataset.cat));
    if (sectionToggleAllBtn) {
      sectionToggleAllBtn.disabled = searchActive;
      sectionToggleAllBtn.dataset.action = anyCollapsed ? "expand" : "collapse";
    }
    if (sectionToggleAllLabel) {
      sectionToggleAllLabel.textContent = anyCollapsed
        ? "Expand all sections"
        : "Collapse all sections";
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

  // Compare dotted version strings numerically: negative when a < b, positive
  // when a > b. Missing components count as 0, so `1.65` < `1.65.1`.
  function cmpVersion(a, b) {
    const pa = a.split(".");
    const pb = b.split(".");
    for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
      const d = (Number(pa[i]) || 0) - (Number(pb[i]) || 0);
      if (d !== 0) return d;
    }
    return 0;
  }

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
    const msrv = msrvFilter ? msrvFilter.value : "";
    const stack = stackFilter ? stackFilter.value : "";
    let visible = 0;
    let highlighted = 0;

    for (const tool of tools) {
      const haystack = tool.dataset.keywords || "";
      const deprecated = tool.dataset.archived === "true";
      const recommended = tool.dataset.recommended === "true";
      const licenses = (tool.dataset.license || "")
        .split(/\s+/)
        .filter(Boolean);
      const stacks = (tool.dataset.stacks || "").split(/\s+/).filter(Boolean);
      const toolMsrv = tool.dataset.msrv || "";
      // MSRV filter keeps tools known to build on the chosen version, i.e. whose
      // MSRV is at most the selection. Tools with unknown MSRV can't be
      // guaranteed, so they drop out while the filter is active.
      const msrvOk = !msrv || (toolMsrv && cmpVersion(toolMsrv, msrv) <= 0);
      const isPick = !!stack && stacks.includes(stack);
      const isBaseline = tool.dataset.baseline === "true";
      // Ordinary filters and the active stack all narrow the result set. A stack
      // keeps its curated picks plus the Everyday Essentials baseline.
      const matches =
        (!skipDeprecated || !deprecated) &&
        (!onlyRecommended || recommended) &&
        (!license || licenses.includes(license)) &&
        msrvOk &&
        (!stack || isPick || isBaseline) &&
        terms.every((t) => haystack.includes(t));
      tool.hidden = !matches;
      tool.classList.toggle("is-stack-pick", matches && isPick);
      tool.classList.toggle(
        "is-baseline",
        matches && !!stack && isBaseline && !isPick,
      );
      if (matches) visible++;
      if (matches && isPick) highlighted++;
    }

    sortRows(sortSelect.value);

    // Hide category sections that end up empty, and disable their entry in the
    // "Jump to category" menu so it can't scroll to nothing.
    for (const cat of categories) {
      const visibleRows = cat.querySelectorAll(".tool:not([hidden])");
      const count = visibleRows.length;
      const any = count > 0;
      cat.hidden = !any;
      const countBadge = cat.querySelector(".category-count");
      if (countBadge) countBadge.textContent = String(count);
      if (categoryJump) {
        const opt = categoryJump.querySelector(
          `option[value="cat-${cat.dataset.cat}"]`,
        );
        if (opt) {
          opt.disabled = !any;
          const title = cat.querySelector(".category-title")?.textContent.trim();
          if (title) opt.textContent = `${title} (${count})`;
        }
      }
    }

    noResults.hidden = visible !== 0;
    if (searchClear) searchClear.hidden = input.value.length === 0;
    renderCollapsed();
    updateStackContext(stack, highlighted);
    updateFilterCount();
    syncUrl();
  }

  // Number of active filters, surfaced as a badge on the mobile "Filters"
  // toggle so the collapsed state still signals that filtering is in effect.
  // "Jump to" is navigation, not a filter, so it doesn't count.
  function updateFilterCount() {
    if (!filtersCount) return;
    let n = 0;
    if (stackFilter && stackFilter.value) n++;
    if (licenseFilter.value) n++;
    if (msrvFilter && msrvFilter.value) n++;
    if (sortSelect.value !== "default") n++;
    if (hideDeprecated.checked) n++;
    if (recommendedOnly.checked) n++;
    filtersCount.textContent = String(n);
    filtersCount.hidden = n === 0;
    // The panel footer (Reset + Apply) is only meaningful once something is
    // filtered, so reveal it on the first active filter.
    if (catalogControls) {
      catalogControls.classList.toggle("has-active-filters", n > 0);
    }
  }

  // Reveal the selected stack's banner and its picks' inline notes, plus the
  // Everyday Essentials baseline notes that ride along under any active stack;
  // everything else (and all of it, when no stack is active) stays hidden.
  function updateStackContext(active, highlighted) {
    for (const banner of stackBanners) {
      banner.hidden = banner.dataset.stack !== active;
    }
    for (const note of stackNotes) {
      const show = note.hasAttribute("data-baseline-note")
        ? !!active
        : note.dataset.stack === active;
      note.hidden = !show;
    }
    for (const option of stackOptions) {
      option.setAttribute(
        "aria-pressed",
        String(option.dataset.stack ? option.dataset.stack === active : !active),
      );
    }
    const selected = active
      ? stackOptions.find((option) => option.dataset.stack === active)
      : null;

    if (stackDropdown) {
      const label = stackDropdown.querySelector(".stack-start-toggle-label");
      const hint = stackDropdown.querySelector(".stack-start-toggle-hint");
      if (label) {
        label.textContent = selected
          ? selected.dataset.stackName
          : label.dataset.defaultLabel || "Choose a stack";
      }
      if (hint) {
        hint.textContent = selected
          ? `${highlighted} picks highlighted — open to choose another`
          : "Browse curated tool sets for your use case";
      }
    }
  }

  // ── Shareable URL state ──────────────────────────────────────────────────
  // Only non-default values are written, so a pristine page stays at `/`.

  function syncUrl() {
    const params = new URLSearchParams();
    if (input.value.trim()) params.set("q", input.value.trim());
    if (licenseFilter.value) params.set("license", licenseFilter.value);
    if (msrvFilter && msrvFilter.value) params.set("msrv", msrvFilter.value);
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
    const msrv = params.get("msrv");
    if (
      msrvFilter &&
      msrv &&
      [...msrvFilter.options].some((o) => o.value === msrv)
    ) {
      msrvFilter.value = msrv;
    }
    const sort = params.get("sort");
    if (sort && comparators[sort]) sortSelect.value = sort;
    hideDeprecated.checked = params.get("deprecated") === "0";
    recommendedOnly.checked = params.get("recommended") === "1";
    const stack = params.get("stack");
    if (
      stackFilter &&
      stack &&
      stackOptions.some((option) => option.dataset.stack === stack)
    ) {
      stackFilter.value = stack;
    }
  }

  restoreFromUrl();
  apply();

  input.addEventListener("input", apply);
  if (searchClear) {
    searchClear.addEventListener("click", () => {
      input.value = "";
      input.focus();
      apply();
    });
  }
  hideDeprecated.addEventListener("change", apply);
  recommendedOnly.addEventListener("change", apply);
  licenseFilter.addEventListener("change", apply);
  if (msrvFilter) msrvFilter.addEventListener("change", apply);
  sortSelect.addEventListener("change", apply);

  // Click a category header (or its chevron) to fold it away. The chevron is a
  // real <button>, so keyboard users get the same toggle for free.
  for (const cat of categories) {
    const head = cat.querySelector(".category-head");
    if (!head) continue;
    // Track where the pointer went down so a click-to-toggle isn't confused
    // with a text drag-select. Inspecting window.getSelection() instead was
    // unreliable (a stale selection left the header stuck, needing several
    // clicks); comparing down/up positions only suppresses a real drag.
    let downX = 0;
    let downY = 0;
    head.addEventListener("mousedown", (e) => {
      downX = e.clientX;
      downY = e.clientY;
    });
    head.addEventListener("click", (e) => {
      if (e.target.closest("a")) return;
      // Keyboard activation (Enter/Space on the toggle button) reports detail 0
      // and carries no coordinates, so always toggle in that case.
      if (e.detail !== 0) {
        const moved =
          Math.abs(e.clientX - downX) > 4 || Math.abs(e.clientY - downY) > 4;
        if (moved) return;
      }
      setCollapsed(cat.dataset.cat, !collapsed.has(cat.dataset.cat));
    });
  }

  // Fold or unfold every visible category at once. If even one is folded, the
  // action expands everything; otherwise it turns the page into a compact index
  // of category headings. The button stays mounted, so keyboard focus is stable.
  if (sectionToggleAllBtn) {
    sectionToggleAllBtn.addEventListener("click", () => {
      const shown = categories.filter((c) => !c.hidden);
      if (sectionToggleAllBtn.dataset.action === "expand") {
        collapsed.clear();
      } else {
        for (const category of shown) collapsed.add(category.dataset.cat);
      }
      persistCollapsed();
      renderCollapsed();
    });
  }

  // On small screens the filter controls collapse behind a "Filters" toggle to
  // keep the catalog near the top; the button just flips a class (the panel and
  // its collapsed state are styled in the mobile media query).
  function closeFilters() {
    if (!catalogControls) return;
    catalogControls.classList.remove("filters-open");
    if (filtersToggle) filtersToggle.setAttribute("aria-expanded", "false");
    requestAnimationFrame(syncStickyOffsets);
  }

  if (filtersToggle && catalogControls) {
    filtersToggle.addEventListener("click", () => {
      const open = catalogControls.classList.toggle("filters-open");
      filtersToggle.setAttribute("aria-expanded", String(open));
      requestAnimationFrame(syncStickyOffsets);
    });
  }

  // "Apply filters" just confirms and closes the panel — filtering itself is
  // already live as each control changes.
  const filtersApply = document.getElementById("filters-apply");
  if (filtersApply) {
    filtersApply.addEventListener("click", () => {
      closeFilters();
      if (filtersToggle) filtersToggle.focus();
    });
  }

  // "Reset all" returns every filter to its default in one tap (search is left
  // alone — it has its own clear button). Live filtering re-runs via apply().
  const filtersReset = document.getElementById("filters-reset");
  if (filtersReset) {
    filtersReset.addEventListener("click", () => {
      if (stackFilter) stackFilter.value = "";
      licenseFilter.value = "";
      if (msrvFilter) msrvFilter.value = "";
      sortSelect.value = "default";
      hideDeprecated.checked = false;
      recommendedOnly.checked = false;
      apply();
    });
  }

  // The stack picker is the single persistent stack control. It compacts once
  // it reaches the sticky header; the catalog toolbar stays directly below it.
  function syncStickyOffsets() {
    if (siteHeader) {
      document.documentElement.style.setProperty(
        "--header-sticky-h",
        siteHeader.offsetHeight + "px",
      );
    }
    if (catalogControls) {
      document.documentElement.style.setProperty(
        "--catalog-controls-sticky-h",
        catalogControls.offsetHeight + "px",
      );
    }
    updateStickyStackSelector();
  }

  function updateStickyStackSelector() {
    if (!stackStart) return;
    const stickyTop = Number.parseFloat(getComputedStyle(stackStart).top) || 0;
    const rect = stackStart.getBoundingClientRect();
    stackStart.classList.toggle(
      "is-compact",
      rect.top <= stickyTop + 1 && rect.bottom > stickyTop,
    );
  }

  let stickySelectorFrame = 0;
  function queueStickySelectorUpdate() {
    if (stickySelectorFrame) return;
    stickySelectorFrame = requestAnimationFrame(() => {
      stickySelectorFrame = 0;
      updateStickyStackSelector();
    });
  }

  syncStickyOffsets();
  window.addEventListener("resize", syncStickyOffsets);
  window.addEventListener("scroll", queueStickySelectorUpdate, { passive: true });

  // Stack selection flows through the hidden input next to the semantic
  // dropdown (the single source of truth for URL sync and row filtering).
  document.addEventListener("click", (e) => {
    const option = e.target.closest(".stack-option[data-stack]");
    if (option && stackFilter) {
      e.preventDefault();
      const id = option.dataset.stack;
      stackFilter.value = id;
      apply();
      if (stackDropdown) stackDropdown.open = false;
      return;
    }
    const chip = e.target.closest(".chip[data-stack]");
    if (chip && stackFilter) {
      e.preventDefault();
      stackFilter.value = chip.dataset.stack;
      apply();
    }
  });

  // Native details/summary provides keyboard disclosure behavior. Add the two
  // conventional dropdown affordances it does not provide itself.
  if (stackDropdown) {
    document.addEventListener("click", (e) => {
      if (stackDropdown.open && !e.target.closest("#stack-dropdown")) {
        stackDropdown.open = false;
      }
    });
    document.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && stackDropdown.open) {
        stackDropdown.open = false;
        stackDropdown.querySelector("summary")?.focus();
      }
    });
  }

  // "Jump to category" is pure navigation: scroll to the chosen section, then
  // reset so the control always reads "Category…".
  if (categoryJump) {
    categoryJump.addEventListener("change", () => {
      const id = categoryJump.value;
      const section = id && document.getElementById(id);
      if (section && !section.hidden) {
        // Unfold the target so the jump lands on content, not a folded header.
        setCollapsed(section.dataset.cat, false);
        // Collapse the (sticky) filter panel first so it doesn't cover the
        // landing spot on mobile.
        closeFilters();
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

  // ── Copy install command ─────────────────────────────────────────────────
  // Tool rows and stack banners expose their install commands through buttons
  // carrying the command in data-copy. Both use the same clipboard behavior and
  // brief inline confirmation.
  document.addEventListener("click", (e) => {
    const btn = e.target.closest(".copy-install");
    if (!btn) return;
    e.preventDefault();
    const command =
      btn.dataset.copy ||
      btn
        .closest(".stack-banner-install")
        ?.querySelector("code")
        ?.textContent.trim() ||
      "";
    if (!command) return;

    const idleLabel = btn.dataset.copyLabel || "install";
    const idleAriaLabel = btn.getAttribute("aria-label");
    const confirm = () => {
      const label = btn.querySelector(".copy-label");
      btn.classList.add("is-copied");
      btn.setAttribute("aria-label", "Copied install command");
      if (label) label.textContent = "copied";
      clearTimeout(btn.copyResetTimer);
      btn.copyResetTimer = setTimeout(() => {
        btn.classList.remove("is-copied");
        if (idleAriaLabel) btn.setAttribute("aria-label", idleAriaLabel);
        else btn.removeAttribute("aria-label");
        if (label) label.textContent = idleLabel;
      }, 1500);
    };

    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(command).then(confirm, fallbackCopy);
    } else {
      fallbackCopy();
    }

    function fallbackCopy() {
      const ta = document.createElement("textarea");
      ta.value = command;
      ta.setAttribute("readonly", "");
      ta.style.position = "absolute";
      ta.style.left = "-9999px";
      document.body.appendChild(ta);
      ta.select();
      try {
        document.execCommand("copy");
        confirm();
      } catch (err) {}
      document.body.removeChild(ta);
    }
  });
})();
