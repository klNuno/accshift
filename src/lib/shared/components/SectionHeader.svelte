<script lang="ts">
  import type { FolderInfo } from "$lib/features/folders/types";

  let {
    folder = null,
    label,
    count,
    cardColor = "",
    accentColor = "#3b82f6",
    collapsed = false,
    showChevron = true,
    showCount = true,
    showFolderIcon = true,
    onToggle,
    onNavigate,
    onContextMenu,
  }: {
    folder?: FolderInfo | null;
    label: string;
    count: number;
    cardColor?: string;
    accentColor?: string;
    collapsed?: boolean;
    showChevron?: boolean;
    showCount?: boolean;
    showFolderIcon?: boolean;
    onToggle?: () => void;
    onNavigate?: (folderId: string) => void;
    onContextMenu?: (e: MouseEvent, folder: FolderInfo) => void;
  } = $props();

  function handleClick() {
    if (onToggle) onToggle();
  }

  function handleDblClick() {
    if (folder && onNavigate) onNavigate(folder.id);
  }

  function handleContextMenu(e: MouseEvent) {
    if (!folder || !onContextMenu) return;
    e.preventDefault();
    e.stopPropagation();
    onContextMenu(e, folder);
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="section-header"
  class:is-root={folder === null}
  class:has-color={!!cardColor}
  class:clickable={!!onToggle}
  class:collapsed
  style={`--section-accent: ${cardColor || accentColor};`}
  onclick={handleClick}
  ondblclick={handleDblClick}
  oncontextmenu={handleContextMenu}
>
  <span class="rule rule-left"></span>
  <span class="content">
    {#if showChevron}
      <svg
        class="chevron"
        class:up={!collapsed}
        width="12"
        height="12"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2.4"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <polyline points="6 9 12 15 18 9" />
      </svg>
    {/if}
    {#if showFolderIcon && folder}
      <svg class="icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
        <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
      </svg>
    {/if}
    <span class="name">{label}</span>
    {#if showCount}
      <span class="count">{count}</span>
    {/if}
  </span>
  <span class="rule rule-right"></span>
</div>

<style>
  .section-header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 4px;
    font-size: 12px;
    font-weight: 500;
    line-height: 1;
    color: var(--fg);
    user-select: none;
    transition: color 120ms ease-out;
  }

  .section-header.clickable {
    cursor: pointer;
  }

  .section-header.clickable:hover {
    color: color-mix(in srgb, var(--section-accent) 65%, var(--fg));
  }

  .content {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .icon {
    flex-shrink: 0;
    color: color-mix(in srgb, var(--section-accent) 70%, var(--fg-muted));
    display: block;
  }

  .chevron {
    flex-shrink: 0;
    color: color-mix(in srgb, var(--section-accent) 60%, var(--fg-muted));
    display: block;
    transition: transform 220ms cubic-bezier(0.4, 0, 0.2, 1);
    transform: rotate(0deg);
  }

  .chevron.up {
    transform: rotate(180deg);
  }

  .name {
    display: inline-flex;
    align-items: center;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 280px;
    line-height: 1;
  }

  .count {
    flex-shrink: 0;
    padding: 2px 7px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--bg-muted) 75%, transparent);
    color: var(--fg-muted);
    font-size: 11px;
    font-weight: 600;
    line-height: 1;
  }

  .has-color .count {
    background: color-mix(in srgb, var(--section-accent) 22%, var(--bg-muted));
    color: color-mix(in srgb, var(--section-accent) 65%, var(--fg));
  }

  .rule {
    flex: 1;
    height: 2px;
    border: none;
    background-image: repeating-linear-gradient(
      to right,
      color-mix(in srgb, var(--section-accent) 55%, var(--border)) 0 6px,
      transparent 6px 12px
    );
    background-size: 100% 2px;
    background-repeat: no-repeat;
    background-position: center;
    align-self: center;
    opacity: 0.85;
  }

  .has-color .rule {
    background-image: repeating-linear-gradient(
      to right,
      color-mix(in srgb, var(--section-accent) 70%, var(--border)) 0 6px,
      transparent 6px 12px
    );
  }

  .is-root .rule {
    background-image: repeating-linear-gradient(
      to right,
      color-mix(in srgb, var(--border) 90%, transparent) 0 6px,
      transparent 6px 12px
    );
    opacity: 0.7;
  }
</style>
