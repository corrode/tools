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
  const stackCards = Array.from(
    document.querySelectorAll(".stack-card[data-stack]"),
  );
  const stackTrigger = document.getElementById("stack-trigger");
  const stackPanel = document.getElementById("stack-panel");
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
      const isPick = !stack || stacks.includes(stack);
      const isBaseline = tool.dataset.baseline === "true";
      // Baseline (Everyday Essentials) tools show under every stack as the
      // assumed groundwork, even when they aren't one of its own picks.
      const matches =
        (!skipDeprecated || !deprecated) &&
        (!onlyRecommended || recommended) &&
        (!license || licenses.includes(license)) &&
        msrvOk &&
        (isPick || isBaseline) &&
        terms.every((t) => haystack.includes(t));
      tool.hidden = !matches;
      // Dim a row that's shown only because it's the baseline: a stack is
      // active and this tool isn't one of that stack's own picks.
      tool.classList.toggle(
        "is-baseline",
        matches && !!stack && isBaseline && !stacks.includes(stack),
      );
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
    if (searchClear) searchClear.hidden = input.value.length === 0;
    renderCollapsed();
    updateStackContext(stack);
    syncUrl();
  }

  // Reveal the selected stack's banner and its picks' inline notes, plus the
  // Everyday Essentials baseline notes that ride along under any active stack;
  // everything else (and all of it, when no stack is active) stays hidden.
  function updateStackContext(active) {
    for (const banner of stackBanners) {
      banner.hidden = banner.dataset.stack !== active;
    }
    for (const note of stackNotes) {
      const show = note.hasAttribute("data-baseline-note")
        ? !!active
        : note.dataset.stack === active;
      note.hidden = !show;
    }
    for (const card of stackCards) {
      card.setAttribute("aria-pressed", String(card.dataset.stack === active));
    }
    if (stackTrigger) {
      const label = stackTrigger.querySelector(".stack-trigger-label");
      const opt =
        active && stackFilter
          ? [...stackFilter.options].find((o) => o.value === active)
          : null;
      if (label) {
        label.textContent = opt
          ? opt.textContent.trim()
          : label.dataset.defaultLabel || "Select stack";
      }
    }
    const panelClear = document.querySelector(".stack-panel-clear");
    if (panelClear) panelClear.hidden = !active;
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
      [...stackFilter.options].some((o) => o.value === stack)
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
  if (stackFilter) stackFilter.addEventListener("change", apply);

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

  // Stack selection flows through the <select> in the catalog controls (the
  // single source of truth for the active stack, URL sync, and row filtering):
  // the header picker cards, the "In <stack>" row chips, and a banner's "Clear
  // filter" button all just set its value and re-apply.
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
      closeStackMenu();
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
      closeStackMenu();
    }
  });

  // ── Stacks popover ───────────────────────────────────────────────────────
  // The header trigger toggles a panel of the curated stack chips; selecting a
  // chip (handled above) or clicking outside / pressing Escape closes it.
  function closeStackMenu() {
    if (!stackTrigger || !stackPanel || stackPanel.hidden) return;
    stackPanel.hidden = true;
    stackTrigger.setAttribute("aria-expanded", "false");
  }
  if (stackTrigger && stackPanel) {
    stackTrigger.addEventListener("click", (e) => {
      e.stopPropagation();
      const open = stackPanel.hidden;
      stackPanel.hidden = !open;
      stackTrigger.setAttribute("aria-expanded", String(open));
    });
    document.addEventListener("click", (e) => {
      if (!stackPanel.hidden && !e.target.closest(".stack-menu")) {
        closeStackMenu();
      }
    });
    document.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && !stackPanel.hidden) {
        closeStackMenu();
        stackTrigger.focus();
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
  // Each installable tool ships a small "install" button carrying its
  // `cargo install …` line in data-copy. Click it to put the command on the
  // clipboard, with a brief inline confirmation.
  let copyResetTimer = null;
  document.addEventListener("click", (e) => {
    const btn = e.target.closest(".copy-install");
    if (!btn) return;
    e.preventDefault();
    const command = btn.dataset.copy || "";
    if (!command) return;

    const confirm = () => {
      const label = btn.querySelector(".copy-label");
      btn.classList.add("is-copied");
      if (label) label.textContent = "copied";
      clearTimeout(copyResetTimer);
      copyResetTimer = setTimeout(() => {
        btn.classList.remove("is-copied");
        if (label) label.textContent = "install";
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
