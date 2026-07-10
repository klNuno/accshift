<script lang="ts">
  import { onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import WaveText from "$lib/shared/components/WaveText.svelte";
  import type { MessageKey, TranslationParams } from "$lib/i18n";

  let {
    renderSuspended,
    afkVersionLabel,
    afkOverlayVisible,
    afkWaveActive,
    motionPaused,
    afkTextRevealDelayMs,
    isPinLocked,
    isPinUnlocking,
    isPinRetryLocked,
    pinAttempt,
    pinError,
    pinCodeLength,
    onPinAttemptChange,
    onPinInputRefChange,
    t,
  }: {
    renderSuspended: boolean;
    afkVersionLabel: string | null;
    afkOverlayVisible: boolean;
    afkWaveActive: boolean;
    motionPaused: boolean;
    afkTextRevealDelayMs: number;
    isPinLocked: boolean;
    isPinUnlocking: boolean;
    isPinRetryLocked: boolean;
    pinAttempt: string;
    pinError: string;
    pinCodeLength: number;
    onPinAttemptChange: (value: string) => void;
    onPinInputRefChange: (node: HTMLInputElement | null) => void;
    t: (key: MessageKey, params?: TranslationParams) => string;
  } = $props();

  let pinInputElement = $state<HTMLInputElement | null>(null);

  $effect(() => {
    onPinInputRefChange(pinInputElement);
  });

  // The lock disables everything behind the overlay, so the PIN field must
  // already hold keyboard focus or the lock is a dead end for keyboard users.
  $effect(() => {
    if (isPinLocked && !isPinUnlocking) pinInputElement?.focus();
  });

  onDestroy(() => {
    onPinInputRefChange(null);
  });

  function handlePinInput(event: Event) {
    onPinAttemptChange((event.currentTarget as HTMLInputElement).value);
  }

  // The overlay covers the titlebar, so it has to provide its own window
  // management: drag anywhere on the backdrop, plus minimize/close buttons.
  function handleOverlayMouseDown(e: MouseEvent) {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button, input, .pin-card")) return;
    void getCurrentWindow().startDragging();
  }

  function minimizeWindow() {
    void invoke("minimize_window");
  }

  function closeWindow() {
    void invoke("close_window");
  }
</script>

<div
  class="afk-version-strip"
  class:visible={Boolean(afkVersionLabel)}
  aria-hidden={!afkVersionLabel}
>
  {#if afkVersionLabel}
    <span>{afkVersionLabel}</span>
  {/if}
</div>

{#if !renderSuspended}
  <div
    class="inactive-overlay"
    class:visible={afkOverlayVisible}
    aria-hidden={!afkOverlayVisible}
  >
    <span class="accshift-text">
      <WaveText
        text="ACCSHIFT"
        active={afkWaveActive && !motionPaused}
        startDelayMs={afkTextRevealDelayMs}
      />
    </span>
  </div>

  {#if isPinLocked || isPinUnlocking || isPinRetryLocked}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="pin-lock-overlay"
      class:unlocking={isPinUnlocking}
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
      <div class="pin-card" role="dialog" aria-label={t("pin.lockedTitle")}>
        <h3>{t("pin.lockedTitle")}</h3>
        <p>{t("pin.lockedPrompt")}</p>
        <input
          bind:this={pinInputElement}
          class="pin-input"
          type="password"
          placeholder={t("pin.placeholder")}
          aria-label={t("pin.lockedPrompt")}
          maxlength={pinCodeLength}
          inputmode="numeric"
          pattern="[0-9]*"
          autocomplete="one-time-code"
          disabled={isPinUnlocking || isPinRetryLocked}
          value={pinAttempt}
          oninput={handlePinInput}
        />
        {#if pinError}
          <span class="pin-error" role="alert">{pinError}</span>
        {/if}
      </div>
    </div>
  {/if}
{/if}

<style>
  .inactive-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
    opacity: 0;
    transition: opacity 900ms ease-in-out;
    transition-delay: 0ms;
    z-index: 300;
  }

  .inactive-overlay.visible {
    opacity: 1;
    transition: opacity 900ms ease-in-out;
    transition-delay: 0ms;
  }

  .accshift-text {
    position: absolute;
    left: 50%;
    top: 50%;
    font-style: normal;
    font-size: clamp(28px, min(13vw, 20vh), 170px);
    line-height: 1;
    letter-spacing: -0.01em;
    white-space: nowrap;
    transform: translate(-50%, -50%);
    user-select: none;
    color: var(--afk-text);
    opacity: 0;
    max-width: 92vw;
    text-align: center;
    transition: opacity 900ms ease-in-out;
    transition-delay: 0ms;
  }

  .inactive-overlay.visible .accshift-text {
    opacity: 0.92;
    transition-delay: var(--afk-reveal-delay, 2500ms);
  }

  .afk-version-strip {
    position: absolute;
    left: 50%;
    bottom: 18px;
    transform: translate(-50%, 8px);
    pointer-events: none;
    user-select: none;
    -webkit-user-select: none;
    opacity: 0;
    z-index: 320;
  }

  /* The strip is centered with a translateX, so a permanent transform
     transition would animate any post-mount layout settle sideways (the
     known ghost-slide pattern). The transition only exists on the visible
     state: entry animates, exit snaps, mount never slides. */
  .afk-version-strip.visible {
    opacity: 0.25;
    transform: translate(-50%, 0);
    transition: opacity 1200ms ease-in-out, transform 1200ms ease-in-out;
    transition-delay: var(--afk-reveal-delay, 2500ms);
  }

  .afk-version-strip span {
    display: inline-block;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
    line-height: 1;
    color: var(--afk-text);
    text-shadow:
      0 0 10px color-mix(in srgb, var(--afk-text) 40%, transparent),
      0 0 24px color-mix(in srgb, var(--afk-text) 34%, transparent);
  }

  .pin-lock-overlay {
    position: absolute;
    inset: 0;
    z-index: 500;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.35);
    opacity: 1;
    transition: opacity 240ms ease-in-out;
    pointer-events: auto;
  }

  .pin-lock-overlay.unlocking {
    opacity: 0;
    pointer-events: none;
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

  .pin-card {
    width: min(320px, 86vw);
    padding: 18px;
    border-radius: 12px;
    border: 1px solid var(--border);
    background: var(--bg-card);
    display: flex;
    flex-direction: column;
    gap: 10px;
    box-shadow: 0 20px 40px rgba(0, 0, 0, 0.45);
  }

  .pin-lock-overlay.unlocking .pin-card {
    opacity: 0;
    transform: translateY(8px) scale(0.98);
    transition: transform 240ms ease-in-out, opacity 240ms ease-in-out;
  }

  .pin-card h3 {
    margin: 0;
    font-size: 15px;
  }

  .pin-card p {
    margin: 0;
    font-size: 12px;
    color: var(--fg-muted);
  }

  .pin-input {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    color: var(--fg);
    font-size: 16px;
    text-align: center;
    letter-spacing: 0.22em;
    padding: 9px 10px;
    outline: none;
  }

  .pin-input:focus {
    border-color: #eab308;
  }

  .pin-error {
    font-size: 11px;
    color: var(--danger, #f87171);
  }

  :global(html[data-motion="reduced"]) .inactive-overlay,
  :global(html[data-motion="reduced"]) .accshift-text,
  :global(html[data-motion="reduced"]) .afk-version-strip.visible,
  :global(html[data-motion="reduced"]) .pin-lock-overlay,
  :global(html[data-motion="reduced"]) .pin-lock-overlay.unlocking .pin-card {
    transition: none;
  }
</style>
