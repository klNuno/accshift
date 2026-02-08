<script lang="ts">
  import { onMount, onDestroy } from "svelte";

  interface MenuItem {
    label: string;
    action: () => void;
    separator?: false;
  }

  interface SeparatorItem {
    separator: true;
  }

  type MenuEntry = MenuItem | SeparatorItem;

  let { items, x, y, onClose }: {
    items: MenuEntry[];
    x: number;
    y: number;
    onClose: () => void;
  } = $props();

  let menuRef = $state<HTMLDivElement | null>(null);
  let adjustedX = $state(x);
  let adjustedY = $state(y);

  // Adjust position to stay within viewport
  onMount(() => {
    if (!menuRef) return;
    const rect = menuRef.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    if (x + rect.width > vw) adjustedX = vw - rect.width - 4;
    if (y + rect.height > vh) adjustedY = vh - rect.height - 4;
  });

  function handleClickOutside(e: MouseEvent) {
    if (menuRef && !menuRef.contains(e.target as Node)) {
      onClose();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }

  onMount(() => {
    // Delay to prevent the same click from closing it
    setTimeout(() => {
      document.addEventListener("mousedown", handleClickOutside);
    }, 0);
    document.addEventListener("keydown", handleKeydown);
  });

  onDestroy(() => {
    document.removeEventListener("mousedown", handleClickOutside);
    document.removeEventListener("keydown", handleKeydown);
  });
</script>

<div
  class="context-menu"
  bind:this={menuRef}
  style="left: {adjustedX}px; top: {adjustedY}px;"
>
  {#each items as item}
    {#if item.separator}
      <div class="separator"></div>
    {:else}
      <button class="menu-item" onclick={() => { item.action(); onClose(); }}>
        {item.label}
      </button>
    {/if}
  {/each}
</div>

<style>
  .context-menu {
    position: fixed;
    z-index: 100;
    min-width: 180px;
    padding: 4px;
    background: #1c1c1f;
    border: 1px solid #27272a;
    border-radius: 6px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    animation: fadeIn 100ms ease-out;
  }

  @keyframes fadeIn {
    from { opacity: 0; transform: scale(0.96); }
    to { opacity: 1; transform: scale(1); }
  }

  .menu-item {
    display: block;
    width: 100%;
    padding: 6px 10px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: #fafafa;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
    transition: background 80ms;
  }

  .menu-item:hover {
    background: #27272a;
  }

  .menu-item:active {
    background: #3f3f46;
  }

  .separator {
    height: 1px;
    margin: 4px 6px;
    background: #27272a;
  }
</style>
