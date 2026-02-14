<script lang="ts">
  import type { ViewMode } from "../viewMode";

  let { mode, onChange }: {
    mode: ViewMode;
    onChange: (mode: ViewMode) => void;
  } = $props();
</script>

<div class="toggle">
  <button
    class="btn"
    class:active={mode === "grid"}
    title="Grid view"
    onclick={() => onChange("grid")}
  >
    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
      <rect x="3" y="3" width="7" height="7" rx="1.5" />
      <rect x="14" y="3" width="7" height="7" rx="1.5" />
      <rect x="3" y="14" width="7" height="7" rx="1.5" />
      <rect x="14" y="14" width="7" height="7" rx="1.5" />
    </svg>
  </button>
  <button
    class="btn"
    class:active={mode === "list"}
    title="List view"
    onclick={() => onChange("list")}
  >
    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
      <rect x="3" y="4" width="18" height="3" rx="1" />
      <rect x="3" y="10.5" width="18" height="3" rx="1" />
      <rect x="3" y="17" width="18" height="3" rx="1" />
    </svg>
  </button>
</div>

<style>
  .toggle {
    display: flex;
    gap: 2px;
    background: var(--bg-muted);
    border-radius: 5px;
    padding: 2px;
  }

  .btn {
    border: none;
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
    padding: 3px 6px;
    border-radius: 3px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 100ms;
  }

  .btn:hover {
    color: var(--fg);
  }

  .btn.active {
    background: var(--bg-card);
    color: var(--fg);
  }
</style>
