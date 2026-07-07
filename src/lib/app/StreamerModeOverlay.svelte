<script lang="ts">
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
</script>

<div class="streamer-overlay" class:visible={active} aria-hidden={!active}>
  <div class="streamer-panel">
    <span class="streamer-title">
      <WaveText text={t("streamer.title")} active={active && !motionPaused} respectReducedMotion={false} />
    </span>
    <p class="streamer-hint">{t("streamer.hint")}</p>
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

  .streamer-hint {
    margin: 0;
    font-size: 13px;
    color: var(--fg-muted);
    max-width: 34ch;
    line-height: 1.4;
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
    background: color-mix(in srgb, var(--bg-card) 88%, #fff 12%);
    color: var(--fg);
    padding: 9px 16px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: border-color 120ms ease-out, background 120ms ease-out;
  }

  .streamer-btn:hover {
    border-color: color-mix(in srgb, var(--fg) 35%, var(--border));
    background: color-mix(in srgb, var(--bg-card) 82%, #fff 18%);
  }

  .streamer-btn.subtle {
    background: transparent;
    color: var(--fg-muted);
  }

  .streamer-btn.subtle:hover {
    color: var(--fg);
  }
</style>
