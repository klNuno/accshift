<script lang="ts">
  import type { FolderInfo } from "./types";

  let { folder, onOpen, onContextMenu, isDragOver = false, isDragged = false, cardColor = "", accentColor = "#3b82f6" }: {
    folder: FolderInfo;
    onOpen: () => void;
    onContextMenu: (e: MouseEvent) => void;
    isDragOver?: boolean;
    isDragged?: boolean;
    cardColor?: string;
    accentColor?: string;
  } = $props();

  function handleContextMenu(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    onContextMenu(e);
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="card"
  class:drag-over={isDragOver}
  class:dragging={isDragged}
  class:custom-color={!!cardColor}
  onclick={onOpen}
  oncontextmenu={handleContextMenu}
  data-folder-id={folder.id}
  style={`--drag-accent: ${accentColor};${cardColor ? ` --folder-custom-color: ${cardColor};` : ""}`}
>
  <div class="icon-wrap">
    <svg width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
      <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
    </svg>
  </div>
  <div class="name">{folder.name}</div>
</div>

<style>
  .card {
    width: var(--grid-card-width);
    min-height: var(--grid-card-min-height);
    padding: var(--grid-card-padding);
    border-radius: var(--grid-card-radius);
    text-align: center;
    background: transparent;
    border: none;
    outline: 1px solid transparent;
    box-shadow: inset 0 0 0 1px transparent;
    cursor: pointer;
    transition: background 180ms ease-out, transform 180ms ease-out, box-shadow 180ms ease-out, outline-color 120ms ease-out;
    color: inherit;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: flex-start;
    box-sizing: border-box;
    user-select: none;
  }

  .card:hover {
    background: var(--bg-card-hover);
    outline-color: color-mix(in srgb, var(--folder-custom-color, var(--fg-subtle)) 45%, transparent);
    transform: translateY(-2px);
    box-shadow: 0 12px 24px rgba(0, 0, 0, 0.18);
  }

  .card.custom-color {
    color: color-mix(in srgb, var(--folder-custom-color) 55%, var(--fg));
  }

  .card.custom-color:hover {
    background: color-mix(in srgb, var(--folder-custom-color) 32%, var(--bg-card-hover));
  }

  .card:active {
    transform: translateY(0) scale(0.985);
  }

  .card.drag-over {
    box-shadow: inset 0 0 0 1px var(--drag-accent, #3b82f6);
    outline-color: color-mix(in srgb, var(--drag-accent, #3b82f6) 55%, transparent);
    background: color-mix(in srgb, var(--drag-accent, #3b82f6) 10%, transparent);
  }

  .card.dragging {
    opacity: 0.4;
    transform: scale(0.95);
  }

  .icon-wrap {
    width: var(--grid-card-avatar-size);
    height: var(--grid-card-avatar-size);
    margin-bottom: 8px;
    border-radius: 6px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--bg-muted) 52%, transparent);
    color: var(--fg-muted);
    transition: background 150ms, transform 180ms ease-out, color 150ms ease-out;
    pointer-events: none;
  }

  .card:hover .icon-wrap {
    background: var(--bg-elevated);
    color: var(--fg);
    transform: translateY(-1px);
  }

  .name {
    font-size: 12px;
    font-weight: 500;
    color: var(--fg);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    width: 100%;
    pointer-events: none;
  }
</style>
