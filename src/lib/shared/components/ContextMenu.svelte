<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import type { ContextMenuItem } from "../types";
  import { DEFAULT_LOCALE, translate, type Locale, type MessageKey } from "$lib/i18n";
  import { trackDependencies } from "$lib/shared/trackDependencies";

  let { items, x, y, onClose, locale = DEFAULT_LOCALE }: {
    items: ContextMenuItem[];
    x: number;
    y: number;
    onClose: () => void;
    locale?: Locale;
  } = $props();

  let menuRef = $state<HTMLDivElement | null>(null);
  let submenuRef = $state<HTMLDivElement | null>(null);
  let adjustedX = $state(0);
  let adjustedY = $state(0);
  let submenuItems = $state<ContextMenuItem[] | null>(null);
  let submenuTop = $state(0);
  let submenuLeft = $state(0);
  let submenuLoading = $state(false);
  let submenuError = $state("");
  let hoveredSubmenuIndex = $state<number | null>(null);
  const EDGE_GAP = 4;
  const SUBMENU_SWITCH_DELAY = 150;
  let submenuSwitchTimer: number | null = null;

  // The menu is position:fixed but .app-shell has will-change: transform (and
  // .app-stage gets a scale() when the UI zoom isn't 100%), so its containing
  // block is that ancestor, not the viewport: left/top live in a scaled local
  // space. Derive that space from the menu itself instead of assuming which
  // ancestor is the containing block: scale from rendered vs layout width, and
  // the viewport position of the local origin from the menu's own coordinates.
  function getLocalSpace() {
    const rect = menuRef!.getBoundingClientRect();
    const scale = menuRef!.offsetWidth > 0 ? rect.width / menuRef!.offsetWidth : 1;
    return {
      scale,
      originLeft: rect.left - adjustedX * scale,
      originTop: rect.top - adjustedY * scale,
    };
  }

  async function positionMenu() {
    await tick();
    if (!menuRef) return;
    const { scale, originLeft, originTop } = getLocalSpace();
    const menuWidth = menuRef.offsetWidth;
    const menuHeight = menuRef.offsetHeight;
    const localX = (x - originLeft) / scale;
    const localY = (y - originTop) / scale;
    const minX = -originLeft / scale + EDGE_GAP;
    const minY = -originTop / scale + EDGE_GAP;
    const maxX = (window.innerWidth - originLeft) / scale - menuWidth - EDGE_GAP;
    const maxY = (window.innerHeight - originTop) / scale - menuHeight - EDGE_GAP;

    adjustedX = Math.max(minX, Math.min(localX, maxX));
    adjustedY = Math.max(minY, Math.min(localY, maxY));
  }

  async function positionSubmenu(anchorTop: number) {
    await tick();
    if (!menuRef) return;
    const menuRect = menuRef.getBoundingClientRect();
    const scale = menuRef.offsetWidth > 0 ? menuRect.width / menuRef.offsetWidth : 1;
    const menuWidth = menuRef.offsetWidth;
    const submenuWidth = submenuRef?.offsetWidth ?? 240;
    const submenuHeight = submenuRef?.offsetHeight ?? 0;
    // Viewport edges expressed in the menu's local coordinates.
    const localTop = -menuRect.top / scale;
    const localRight = (window.innerWidth - menuRect.left) / scale;
    const localBottom = (window.innerHeight - menuRect.top) / scale;

    const canOpenRight = menuWidth + 4 + submenuWidth <= localRight - EDGE_GAP;
    submenuLeft = canOpenRight ? menuWidth + 4 : -(submenuWidth + 4);

    let top = anchorTop;
    if (submenuHeight > 0 && top + submenuHeight > localBottom - EDGE_GAP) {
      top = localBottom - submenuHeight - EDGE_GAP;
    }
    if (top < localTop + EDGE_GAP) top = localTop + EDGE_GAP;
    submenuTop = top;
  }

  onMount(() => {
    void positionMenu().then(() => {
      focusableItems("menu")[0]?.focus({ preventScroll: true });
    });
  });

  $effect(() => {
    trackDependencies(x, y);
    void positionMenu();
  });

  function handleClickOutside(e: MouseEvent) {
    if (menuRef && !menuRef.contains(e.target as Node)) {
      onClose();
    }
  }

  function handleScroll(e: Event) {
    // Scrolling inside the menu (the submenu can overflow) must not close it.
    if (menuRef && e.target instanceof Node && menuRef.contains(e.target)) return;
    onClose();
  }

  function focusableItems(scope: "menu" | "submenu"): HTMLElement[] {
    if (scope === "submenu") {
      return submenuRef
        ? Array.from(submenuRef.querySelectorAll<HTMLElement>("[data-menu-item]"))
        : [];
    }
    if (!menuRef) return [];
    return Array.from(menuRef.querySelectorAll<HTMLElement>("[data-menu-item]")).filter(
      (el) => !submenuRef?.contains(el),
    );
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      onClose();
      return;
    }
    const active = document.activeElement as HTMLElement | null;
    const inSubmenu = !!(submenuRef && active && submenuRef.contains(active));

    switch (e.key) {
      case "ArrowDown":
      case "ArrowUp": {
        e.preventDefault();
        const list = focusableItems(inSubmenu ? "submenu" : "menu");
        if (list.length === 0) return;
        const current = active ? list.indexOf(active) : -1;
        const delta = e.key === "ArrowDown" ? 1 : -1;
        const next =
          current === -1
            ? delta === 1
              ? 0
              : list.length - 1
            : (current + delta + list.length) % list.length;
        list[next].focus({ preventScroll: false });
        return;
      }
      case "Home":
      case "End": {
        e.preventDefault();
        const list = focusableItems(inSubmenu ? "submenu" : "menu");
        if (list.length === 0) return;
        (e.key === "Home" ? list[0] : list[list.length - 1]).focus();
        return;
      }
      case "ArrowRight": {
        if (inSubmenu || !active || active.dataset.submenu !== "true") return;
        e.preventDefault();
        const index = Number(active.dataset.index);
        const item = items[index];
        if (!item) return;
        if (hoveredSubmenuIndex === index && submenuItems) {
          focusableItems("submenu")[0]?.focus();
          return;
        }
        void openSubmenuForKeyboard(item, index, active);
        return;
      }
      case "ArrowLeft": {
        if (!inSubmenu) return;
        e.preventDefault();
        const parentIndex = hoveredSubmenuIndex;
        clearSubmenu();
        if (parentIndex !== null) {
          focusableItems("menu")
            .find((el) => el.dataset.index === String(parentIndex))
            ?.focus();
        }
        return;
      }
      case "Enter":
      case " ": {
        if (active && menuRef?.contains(active) && active.dataset.menuItem !== undefined) {
          e.preventDefault();
          active.click();
        }
        return;
      }
    }
  }

  function clearSubmenu() {
    cancelSubmenuSwitch();
    hoveredSubmenuIndex = null;
    submenuItems = null;
    submenuLoading = false;
    submenuError = "";
  }

  function cancelSubmenuSwitch() {
    if (submenuSwitchTimer !== null) {
      clearTimeout(submenuSwitchTimer);
      submenuSwitchTimer = null;
    }
  }

  // Hover intent: while a submenu is open, moving diagonally toward it crosses
  // the item below, which would instantly replace or close it. Delay the
  // switch; reaching the submenu (or coming back) cancels the timer.
  function scheduleSubmenuChange(fn: () => void) {
    cancelSubmenuSwitch();
    if (hoveredSubmenuIndex === null) {
      fn();
      return;
    }
    submenuSwitchTimer = window.setTimeout(() => {
      submenuSwitchTimer = null;
      fn();
    }, SUBMENU_SWITCH_DELAY);
  }

  function handleItemMouseEnter(item: ContextMenuItem, index: number, el: HTMLElement) {
    if (hoveredSubmenuIndex === index) {
      cancelSubmenuSwitch();
      return;
    }
    if (item.submenu || item.submenuLoader) {
      scheduleSubmenuChange(() => void openSubmenu(item, index, el));
    } else {
      scheduleSubmenuChange(clearSubmenu);
    }
  }

  function handlePlainItemMouseEnter() {
    scheduleSubmenuChange(clearSubmenu);
  }

  async function openSubmenu(item: ContextMenuItem, index: number, anchor: HTMLElement) {
    cancelSubmenuSwitch();
    if (!item.submenu && !item.submenuLoader) {
      clearSubmenu();
      return;
    }
    hoveredSubmenuIndex = index;
    const anchorTop = anchor.offsetTop;
    submenuTop = anchorTop;
    submenuError = "";
    void positionSubmenu(anchorTop);
    if (item.submenu) {
      submenuLoading = false;
      submenuItems = item.submenu;
      void positionSubmenu(anchorTop);
      return;
    }
    if (!item.submenuLoader) return;
    submenuLoading = true;
    submenuItems = null;
    try {
      const loaded = await item.submenuLoader();
      // Drop stale results if another item was hovered while loading.
      if (hoveredSubmenuIndex !== index) return;
      submenuItems = loaded;
    } catch (e) {
      if (hoveredSubmenuIndex !== index) return;
      console.error("Context menu submenu failed to load:", e);
      submenuError = translate(locale, "context.submenuError" as MessageKey);
      submenuItems = [];
    } finally {
      if (hoveredSubmenuIndex === index) {
        submenuLoading = false;
        void positionSubmenu(anchorTop);
      }
    }
  }

  async function openSubmenuForKeyboard(item: ContextMenuItem, index: number, anchor: HTMLElement) {
    await openSubmenu(item, index, anchor);
    await tick();
    if (hoveredSubmenuIndex !== index) return;
    focusableItems("submenu")[0]?.focus({ preventScroll: true });
  }

  onMount(() => {
    document.addEventListener("mousedown", handleClickOutside);
    document.addEventListener("keydown", handleKeydown);
    document.addEventListener("scroll", handleScroll, true);
    window.addEventListener("resize", positionMenu);
  });

  onDestroy(() => {
    document.removeEventListener("mousedown", handleClickOutside);
    document.removeEventListener("keydown", handleKeydown);
    document.removeEventListener("scroll", handleScroll, true);
    window.removeEventListener("resize", positionMenu);
    cancelSubmenuSwitch();
  });
</script>

<div
  class="context-menu"
  role="menu"
  tabindex="-1"
  bind:this={menuRef}
  style="left: {adjustedX}px; top: {adjustedY}px;"
>
  {#each items as item, idx}
    {#if item.separator}
      <div class="separator" role="separator"></div>
    {:else if item.swatches}
      <div class="swatch-group" role="group" aria-label={item.label}>
        <div class="swatch-label">{item.label}</div>
        <div class="swatch-row">
          {#each item.swatches as sw}
            <button
              class="swatch"
              class:active={sw.active}
              role="menuitem"
              tabindex="-1"
              data-menu-item
              title={sw.label}
              aria-label={sw.label}
              onmouseenter={handlePlainItemMouseEnter}
              onclick={() => { sw.action(); onClose(); }}
            >
              {#if sw.color}
                <span class="swatch-fill" style={`background:${sw.color};`}></span>
              {:else}
                <span class="swatch-fill default"></span>
              {/if}
            </button>
          {/each}
        </div>
      </div>
    {:else}
      <button
        class="menu-item"
        role="menuitem"
        tabindex="-1"
        data-menu-item
        data-index={idx}
        data-submenu={item.submenu || item.submenuLoader ? "true" : undefined}
        aria-haspopup={item.submenu || item.submenuLoader ? "menu" : undefined}
        aria-expanded={item.submenu || item.submenuLoader ? hoveredSubmenuIndex === idx : undefined}
        onmouseenter={(e) => handleItemMouseEnter(item, idx, e.currentTarget)}
        onclick={(e) => {
          if (item.action) {
            item.action();
            onClose();
            return;
          }
          if (item.submenu || item.submenuLoader) {
            void openSubmenu(item, idx, e.currentTarget);
            return;
          }
          onClose();
        }}
      >
        <span>{item.label}</span>
        {#if item.submenu || item.submenuLoader}
          <span class="submenu-arrow">›</span>
        {/if}
      </button>
    {/if}
  {/each}

  {#if hoveredSubmenuIndex !== null}
    <div
      class="submenu"
      role="menu"
      tabindex="-1"
      bind:this={submenuRef}
      style={`top:${submenuTop}px; left:${submenuLeft}px;`}
      onmouseenter={cancelSubmenuSwitch}
    >
      {#if submenuLoading}
        <div class="submenu-state">{translate(locale, "context.loading")}</div>
      {:else if submenuError}
        <div class="submenu-state error">{submenuError}</div>
      {:else if !submenuItems || submenuItems.length === 0}
        <div class="submenu-state">{translate(locale, "context.noGamesFound")}</div>
      {:else}
        {#each submenuItems as sub}
          {#if sub.separator}
            <div class="separator" role="separator"></div>
          {:else if sub.swatches}
            <div class="swatch-group submenu-swatch-group" role="group" aria-label={sub.label}>
              <div class="swatch-label">{sub.label}</div>
              <div class="swatch-row">
                {#each sub.swatches as sw}
                  <button
                    class="swatch"
                    class:active={sw.active}
                    role="menuitem"
                    tabindex="-1"
                    data-menu-item
                    title={sw.label}
                    aria-label={sw.label}
                    onclick={() => { sw.action(); onClose(); }}
                  >
                    {#if sw.color}
                      <span class="swatch-fill" style={`background:${sw.color};`}></span>
                    {:else}
                      <span class="swatch-fill default"></span>
                    {/if}
                  </button>
                {/each}
              </div>
            </div>
          {:else}
            <button
              class="menu-item"
              role="menuitem"
              tabindex="-1"
              data-menu-item
              onclick={() => { sub.action?.(); onClose(); }}
            >
              {sub.label}
            </button>
          {/if}
        {/each}
      {/if}
    </div>
  {/if}
</div>

<style>
  .context-menu {
    position: fixed;
    z-index: 1200;
    min-width: 220px;
    padding: 4px;
    background: var(--bg-overlay);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
  }

  .menu-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 6px 10px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--fg);
    font-size: 12px;
    text-align: left;
    cursor: pointer;
    transition: background 80ms;
  }

  .menu-item:hover,
  .menu-item:focus-visible {
    background: var(--bg-muted);
    outline: none;
  }

  .menu-item:active {
    background: var(--bg-elevated);
  }

  .submenu-arrow {
    color: var(--fg-subtle);
    margin-left: 8px;
  }

  .separator {
    height: 1px;
    margin: 4px 6px;
    background: var(--border);
  }

  .swatch-group {
    padding: 6px 8px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .swatch-label {
    font-size: 10px;
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .swatch-row {
    display: flex;
    gap: 6px;
    flex-wrap: nowrap;
  }

  .swatch {
    width: 18px;
    height: 18px;
    padding: 0;
    border-radius: 4px;
    border: 1px solid var(--border);
    background: var(--bg-solid);
    cursor: pointer;
    display: grid;
    place-items: center;
  }

  .swatch:hover,
  .swatch:focus-visible {
    outline: none;
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--fg) 35%, transparent);
  }

  .swatch-fill {
    width: 12px;
    height: 12px;
    border-radius: 3px;
    display: block;
  }

  .swatch-fill.default {
    background: linear-gradient(135deg, var(--bg-muted) 0 50%, var(--bg-elevated) 50% 100%);
  }

  .swatch.active {
    border-color: var(--fg);
    box-shadow: 0 0 0 1px var(--fg);
  }

  .submenu {
    position: absolute;
    min-width: 240px;
    max-width: min(320px, calc(100vw - 8px));
    max-height: calc(100vh - 8px);
    overflow-y: auto;
    padding: 4px;
    background: var(--bg-overlay);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
  }

  .submenu-swatch-group {
    padding-top: 4px;
  }

  .submenu-state {
    font-size: 12px;
    color: var(--fg-muted);
    padding: 8px;
  }

  .submenu-state.error {
    color: #f87171;
  }
</style>
