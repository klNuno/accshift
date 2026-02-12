<script lang="ts">
  import type { FolderInfo } from "./types";

  let { folder, onOpen, onContextMenu, isDragOver = false, isDragged = false }: {
    folder: FolderInfo;
    onOpen: () => void;
    onContextMenu: (e: MouseEvent) => void;
    isDragOver?: boolean;
    isDragged?: boolean;
  } = $props();

  function handleContextMenu(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    onContextMenu(e);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="card"
  class:drag-over={isDragOver}
  class:dragging={isDragged}
  onclick={onOpen}
  oncontextmenu={handleContextMenu}
  data-folder-id={folder.id}
>
  <div class="icon-wrap">
    <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
      <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
    </svg>
  </div>
  <div class="name">{folder.name}</div>
</div>

<style>
  .card {
    width: 120px;
    padding: 12px;
    border-radius: 8px;
    text-align: center;
    background: var(--bg-card);
    border: 2px solid transparent;
    cursor: pointer;
    transition: all 150ms ease-out;
    color: inherit;
    display: flex;
    flex-direction: column;
    align-items: center;
    box-sizing: border-box;
    user-select: none;
  }

  .card:hover {
    background: var(--bg-card-hover);
    transform: scale(1.02);
  }

  .card:active {
    transform: scale(0.98);
  }

  .card.drag-over {
    border-color: #3b82f6;
    background: rgba(59, 130, 246, 0.1);
  }

  .card.dragging {
    opacity: 0.4;
    transform: scale(0.95);
  }

  .icon-wrap {
    width: 80px;
    height: 80px;
    margin-bottom: 10px;
    border-radius: 6px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-muted);
    color: var(--fg-muted);
    transition: all 150ms;
    pointer-events: none;
  }

  .card:hover .icon-wrap {
    background: var(--bg-elevated);
    color: var(--fg);
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
