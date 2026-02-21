<script lang="ts">
  import type { ViewMode } from "../viewMode";
  import { DEFAULT_LOCALE, translate, type Locale } from "$lib/i18n";

  let { mode, onChange, locale = DEFAULT_LOCALE }: {
    mode: ViewMode;
    onChange: (mode: ViewMode) => void;
    locale?: Locale;
  } = $props();
</script>

<div class="view-toggle">
  <button
    class="btn"
    class:active={mode === "grid"}
    title={translate(locale, "view.grid")}
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
    title={translate(locale, "view.list")}
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
  .view-toggle {
    display: flex;
    gap: 2px;
    background: var(--bg-muted);
    border-radius: 8px;
    padding: 2px;
    height: 30px;
    box-sizing: border-box;
  }

  .btn {
    border: none;
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
    padding: 0 8px;
    height: 100%;
    border-radius: 6px;
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
