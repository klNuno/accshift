<script lang="ts">
  import type { FolderInfo } from "./types";

  let { platformName, path, onNavigate, accentColor }: {
    platformName: string;
    path: FolderInfo[];
    onNavigate: (folderId: string | null) => void;
    accentColor: string;
  } = $props();
</script>

<div class="breadcrumb">
  <button
    class="crumb"
    class:current={path.length === 0}
    onclick={() => onNavigate(null)}
    style={path.length === 0 ? `color: ${accentColor};` : ""}
  >
    {platformName}
  </button>

  {#each path as folder, i}
    <span class="sep">/</span>
    <button
      class="crumb"
      class:current={i === path.length - 1}
      onclick={() => onNavigate(folder.id)}
      style={i === path.length - 1 ? `color: ${accentColor};` : ""}
    >
      {folder.name}
    </button>
  {/each}
</div>

<style>
  .breadcrumb {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 0 8px 0;
    font-size: 12px;
    min-height: 18px;
    flex-wrap: wrap;
  }

  .crumb {
    border: none;
    background: transparent;
    color: var(--fg-subtle);
    font-size: 12px;
    cursor: pointer;
    padding: 2px 4px;
    border-radius: 3px;
    transition: all 100ms;
  }

  .crumb:hover {
    background: var(--bg-muted);
    color: var(--fg);
  }

  .crumb.current {
    font-weight: 600;
    cursor: default;
  }

  .crumb.current:hover {
    background: transparent;
  }

  .sep {
    color: var(--bg-elevated);
    font-size: 11px;
    user-select: none;
  }
</style>
