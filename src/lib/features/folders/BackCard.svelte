<script lang="ts">
  import { DEFAULT_LOCALE, translate, type Locale } from "$lib/i18n";

  let { onBack, isDragOver = false, locale = DEFAULT_LOCALE }: {
    onBack: () => void;
    isDragOver?: boolean;
    locale?: Locale;
  } = $props();
</script>

<button
  class="card"
  class:drag-over={isDragOver}
  onclick={onBack}
  data-back-card="true"
>
  <div class="icon-wrap">
    <svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <path d="M19 12H5" />
      <path d="M12 19l-7-7 7-7" />
    </svg>
  </div>
  <div class="name">{translate(locale, "common.back")}</div>
</button>

<style>
  .card {
    width: 100px;
    padding: 8px;
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

  .icon-wrap {
    width: 68px;
    height: 68px;
    margin-bottom: 8px;
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
    color: var(--fg-muted);
    pointer-events: none;
  }
</style>
