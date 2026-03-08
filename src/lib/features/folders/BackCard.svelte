<script lang="ts">
  import { DEFAULT_LOCALE, translate, type Locale } from "$lib/i18n";

  let { onBack, isDragOver = false, locale = DEFAULT_LOCALE, accentColor = "#3b82f6" }: {
    onBack: () => void;
    isDragOver?: boolean;
    locale?: Locale;
    accentColor?: string;
  } = $props();
</script>

<button
  class="card"
  class:drag-over={isDragOver}
  onclick={onBack}
  data-back-card="true"
  style={`--drag-accent: ${accentColor};`}
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
    width: var(--grid-card-width);
    min-height: var(--grid-card-min-height);
    padding: var(--grid-card-padding);
    border-radius: var(--grid-card-radius);
    text-align: center;
    background: transparent;
    border: none;
    outline: 1px solid transparent;
    box-shadow: inset 0 0 0 1px transparent;
    appearance: none;
    cursor: pointer;
    transition: transform 150ms ease-out, background 150ms ease-out, outline-color 150ms ease-out, box-shadow 150ms ease-out;
    color: inherit;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: flex-start;
    box-sizing: border-box;
  }

  .card:hover {
    background: color-mix(in srgb, var(--bg-card) 30%, transparent);
    outline-color: color-mix(in srgb, var(--fg-subtle) 45%, transparent);
    transform: scale(1.02);
  }

  .card:active {
    transform: scale(0.98);
  }

  .card.drag-over {
    box-shadow: inset 0 0 0 1px var(--drag-accent, #3b82f6);
    outline-color: color-mix(in srgb, var(--drag-accent, #3b82f6) 55%, transparent);
    background: color-mix(in srgb, var(--drag-accent, #3b82f6) 10%, transparent);
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
    transition: all 150ms;
    pointer-events: none;
  }

  .card:hover .icon-wrap {
    background: color-mix(in srgb, var(--bg-elevated) 72%, transparent);
    color: var(--fg);
  }

  .name {
    font-size: 12px;
    font-weight: 500;
    color: var(--fg-muted);
    pointer-events: none;
  }
</style>
