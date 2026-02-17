<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import type { ContextMenuItem } from "../types";

  let { items, x, y, onClose }: {
    items: ContextMenuItem[];
    x: number;
    y: number;
    onClose: () => void;
  } = $props();

  let menuRef = $state<HTMLDivElement | null>(null);
  let submenuRef = $state<HTMLDivElement | null>(null);
  let adjustedX = $state(x);
  let adjustedY = $state(y);
  let submenuItems = $state<ContextMenuItem[] | null>(null);
  let submenuTop = $state(0);
  let submenuLeft = $state(0);
  let submenuLoading = $state(false);
  let submenuError = $state("");
  let hoveredSubmenuIndex = $state<number | null>(null);
  const EDGE_GAP = 4;

  async function positionMenu() {
    await tick();
    if (!menuRef) return;
    const rect = menuRef.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    adjustedX = Math.max(EDGE_GAP, Math.min(x, vw - rect.width - EDGE_GAP));
    adjustedY = Math.max(EDGE_GAP, Math.min(y, vh - rect.height - EDGE_GAP));
  }

  async function positionSubmenu(anchorTop: number) {
    await tick();
    if (!menuRef) return;
    const menuRect = menuRef.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const submenuWidth = submenuRef?.offsetWidth ?? 240;
    const submenuHeight = submenuRef?.offsetHeight ?? 0;

    const canOpenRight = menuRect.right + 4 + submenuWidth <= vw - EDGE_GAP;
    submenuLeft = canOpenRight ? menuRect.width + 4 : -(submenuWidth + 4);

    const desiredViewportTop = menuRect.top + anchorTop;
    let viewportTop = desiredViewportTop;
    if (submenuHeight > 0 && viewportTop + submenuHeight > vh - EDGE_GAP) {
      viewportTop = Math.max(EDGE_GAP, vh - submenuHeight - EDGE_GAP);
    }
    if (viewportTop < EDGE_GAP) viewportTop = EDGE_GAP;
    submenuTop = viewportTop - menuRect.top;
  }

  onMount(() => {
    void positionMenu();
  });

  function handleClickOutside(e: MouseEvent) {
    if (menuRef && !menuRef.contains(e.target as Node)) {
      onClose();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }

  function clearSubmenu() {
    hoveredSubmenuIndex = null;
    submenuItems = null;
    submenuLoading = false;
    submenuError = "";
  }

  async function openSubmenu(item: ContextMenuItem, index: number, event: MouseEvent) {
    if (!item.submenu && !item.submenuLoader) {
      clearSubmenu();
      return;
    }
    hoveredSubmenuIndex = index;
    const anchorTop = (event.currentTarget as HTMLElement).offsetTop;
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
      submenuItems = await item.submenuLoader();
    } catch (e) {
      submenuError = String(e);
      submenuItems = [];
    } finally {
      submenuLoading = false;
      void positionSubmenu(anchorTop);
    }
  }

  onMount(() => {
    document.addEventListener("mousedown", handleClickOutside);
    document.addEventListener("keydown", handleKeydown);
    window.addEventListener("resize", positionMenu);
  });

  onDestroy(() => {
    document.removeEventListener("mousedown", handleClickOutside);
    document.removeEventListener("keydown", handleKeydown);
    window.removeEventListener("resize", positionMenu);
  });
</script>

<div
  class="context-menu"
  bind:this={menuRef}
  style="left: {adjustedX}px; top: {adjustedY}px;"
>
  {#each items as item, idx}
    {#if item.separator}
      <div class="separator"></div>
    {:else if item.swatches}
      <div class="swatch-group">
        <div class="swatch-label">{item.label}</div>
        <div class="swatch-row">
          {#each item.swatches as sw}
            <button
              class="swatch"
              class:active={sw.active}
              title={sw.label}
              aria-label={sw.label}
              onmouseenter={clearSubmenu}
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
        onmouseenter={(e) => openSubmenu(item, idx, e)}
        onclick={() => {
          if (item.submenu || item.submenuLoader) return;
          item.action?.();
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
    <div class="submenu" bind:this={submenuRef} style={`top:${submenuTop}px; left:${submenuLeft}px;`}>
      {#if submenuLoading}
        <div class="submenu-state">Loading...</div>
      {:else if submenuError}
        <div class="submenu-state error">{submenuError}</div>
      {:else if !submenuItems || submenuItems.length === 0}
        <div class="submenu-state">No games found</div>
      {:else}
        {#each submenuItems as sub}
          <button class="menu-item" onclick={() => { sub.action?.(); onClose(); }}>
            {sub.label}
          </button>
        {/each}
      {/if}
    </div>
  {/if}
</div>

<style>
  .context-menu {
    position: fixed;
    z-index: 100;
    min-width: 220px;
    padding: 4px;
    background: var(--bg-card);
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

  .menu-item:hover {
    background: var(--bg-muted);
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
    background: var(--bg);
    cursor: pointer;
    display: grid;
    place-items: center;
  }

  .swatch-fill {
    width: 12px;
    height: 12px;
    border-radius: 3px;
    display: block;
  }

  .swatch-fill.default {
    background: linear-gradient(135deg, #27272a 0 50%, #3f3f46 50% 100%);
  }

  .swatch.active {
    border-color: #e4e4e7;
    box-shadow: 0 0 0 1px #e4e4e7;
  }

  .submenu {
    position: absolute;
    min-width: 240px;
    max-width: min(320px, calc(100vw - 8px));
    max-height: calc(100vh - 8px);
    overflow-y: auto;
    padding: 4px;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
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
