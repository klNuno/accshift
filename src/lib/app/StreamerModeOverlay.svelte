<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import WaveText from "$lib/shared/components/WaveText.svelte";
  import type { MessageKey, TranslationParams } from "$lib/i18n";

  let {
    active,
    motionPaused,
    onDismiss,
    onDisablePermanently,
    t,
  }: {
    active: boolean;
    motionPaused: boolean;
    onDismiss: () => void;
    onDisablePermanently: () => void;
    t: (key: MessageKey, params?: TranslationParams) => string;
  } = $props();

  // The overlay covers the titlebar, so it provides its own window
  // management: drag on the backdrop, plus minimize/close buttons.
  function handleOverlayMouseDown(e: MouseEvent) {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button")) return;
    void getCurrentWindow().startDragging();
  }

  function minimizeWindow() {
    void invoke("minimize_window");
  }

  function closeWindow() {
    void invoke("close_window");
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="streamer-overlay"
  class:visible={active}
  aria-hidden={!active}
  inert={!active}
  onmousedown={handleOverlayMouseDown}
>
  <div class="overlay-win-btns">
    <button type="button" class="overlay-win-btn" onclick={minimizeWindow} title={t("titlebar.minimize")} aria-label={t("titlebar.minimize")}>
      <svg width="12" height="12" viewBox="0 0 12 12">
        <rect x="1" y="5.5" width="10" height="1" fill="currentColor" />
      </svg>
    </button>
    <button type="button" class="overlay-win-btn close" onclick={closeWindow} title={t("titlebar.close")} aria-label={t("titlebar.close")}>
      <svg width="12" height="12" viewBox="0 0 12 12">
        <path d="M1 1l10 10M11 1L1 11" stroke="currentColor" stroke-width="1.2" />
      </svg>
    </button>
  </div>
  <div class="streamer-panel">
    <span class="streamer-title">
      <WaveText text={t("streamer.title")} active={active && !motionPaused} />
    </span>
    <div class="streamer-actions">
      <button type="button" class="streamer-btn" onclick={onDismiss}>
        {t("streamer.disable")}
      </button>
      <button type="button" class="streamer-btn subtle" onclick={onDisablePermanently}>
        {t("streamer.disablePermanently")}
      </button>
    </div>
  </div>
</div>

<style>
  .streamer-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--bg) 72%, transparent);
    opacity: 0;
    pointer-events: none;
    transition: opacity 320ms ease-in-out;
    z-index: 400;
  }

  .streamer-overlay.visible {
    opacity: 1;
    pointer-events: auto;
  }

  :global(html[data-motion="reduced"]) .streamer-overlay {
    transition: none;
  }

  .overlay-win-btns {
    position: absolute;
    top: 0;
    right: 0;
    display: flex;
  }

  .overlay-win-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    border: none;
    background: transparent;
    color: var(--fg-muted);
    opacity: 0.7;
    cursor: pointer;
    transition: background 120ms;
  }

  .overlay-win-btn:hover {
    background: var(--bg-muted);
    color: var(--fg);
    opacity: 1;
  }

  .overlay-win-btn.close:hover {
    background: var(--danger, #dc2626);
  }

  .streamer-panel {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 18px;
    padding: 0 24px;
    text-align: center;
    user-select: none;
  }

  .streamer-title {
    font-size: clamp(26px, min(11vw, 16vh), 120px);
    line-height: 1;
    letter-spacing: -0.01em;
    white-space: nowrap;
    color: var(--afk-text);
  }

  .streamer-actions {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
    justify-content: center;
  }

  .streamer-btn {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: color-mix(in srgb, var(--bg-card) 88%, var(--fg) 12%);
    color: var(--fg);
    padding: 9px 16px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: border-color 120ms ease-out, background 120ms ease-out;
  }

  .streamer-btn:hover {
    border-color: color-mix(in srgb, var(--fg) 35%, var(--border));
    background: color-mix(in srgb, var(--bg-card) 82%, var(--fg) 18%);
  }

  .streamer-btn.subtle {
    background: transparent;
    color: var(--fg-muted);
  }

  .streamer-btn.subtle:hover {
    color: var(--fg);
  }
</style>
