<script lang="ts">
  import { onDestroy } from "svelte";
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

  onDestroy(() => {
    onPinInputRefChange(null);
  });

  function handlePinInput(event: Event) {
    onPinAttemptChange((event.currentTarget as HTMLInputElement).value);
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
        respectReducedMotion={false}
        startDelayMs={afkTextRevealDelayMs}
      />
    </span>
  </div>

  {#if isPinLocked || isPinUnlocking || isPinRetryLocked}
    <div class="pin-lock-overlay" class:unlocking={isPinUnlocking}>
      <div class="pin-card">
        <h3>{t("pin.lockedTitle")}</h3>
        <p>{t("pin.lockedPrompt")}</p>
        <input
          bind:this={pinInputElement}
          class="pin-input"
          type="password"
          placeholder={t("pin.placeholder")}
          maxlength={pinCodeLength}
          inputmode="numeric"
          pattern="[0-9]*"
          autocomplete="one-time-code"
          disabled={isPinUnlocking || isPinRetryLocked}
          value={pinAttempt}
          oninput={handlePinInput}
        />
        {#if pinError}
          <span class="pin-error">{pinError}</span>
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
    transition: opacity 1200ms ease-in-out, transform 1200ms ease-in-out;
    transition-delay: 0ms;
    z-index: 320;
  }

  .afk-version-strip.visible {
    opacity: 0.25;
    transform: translate(-50%, 0);
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
    transition: transform 240ms ease-in-out, opacity 240ms ease-in-out;
  }

  .pin-lock-overlay.unlocking .pin-card {
    opacity: 0;
    transform: translateY(8px) scale(0.98);
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
    color: #f87171;
  }
</style>
