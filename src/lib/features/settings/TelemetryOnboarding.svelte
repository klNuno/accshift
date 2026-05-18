<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { MessageKey, TranslationParams } from "$lib/i18n";

  let {
    t,
    onComplete,
  }: {
    t: (key: MessageKey, params?: TranslationParams) => string;
    onComplete: () => void;
  } = $props();

  let modeBChecked = $state(false);
  let submitting = $state(false);

  async function handleContinue() {
    if (submitting) return;
    submitting = true;
    try {
      await invoke("telemetry_complete_onboarding", { modeBEnabled: modeBChecked });
      onComplete();
    } catch (e) {
      console.error("telemetry_complete_onboarding failed", e);
      // Do not block the user on error. onboarding_completed stays false so
      // we retry on the next launch.
      onComplete();
    } finally {
      submitting = false;
    }
  }
</script>

<div class="backdrop">
  <div class="modal" role="dialog" aria-modal="true" aria-labelledby="telemetry-onboarding-title">
    <h2 id="telemetry-onboarding-title">{t("onboarding.telemetry.title")}</h2>
    <p class="intro">{t("onboarding.telemetry.intro")}</p>

    <label class="check">
      <input type="checkbox" bind:checked={modeBChecked} />
      <span>{t("onboarding.telemetry.modeBLabel")}</span>
    </label>
    <p class="hint">{t("onboarding.telemetry.modeBHint")}</p>

    <p class="opt-out">{t("onboarding.telemetry.optOutHint")}</p>

    <div class="actions">
      <button type="button" class="primary" disabled={submitting} onclick={handleContinue}>
        {t("onboarding.telemetry.continue")}
      </button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: color-mix(in srgb, #000 60%, transparent);
    backdrop-filter: blur(8px);
    z-index: 9000;
    animation: fadeIn 140ms ease-out;
  }

  .modal {
    width: min(92vw, 480px);
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 14px;
    padding: 22px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    color: var(--fg);
    box-shadow: 0 20px 48px rgba(0, 0, 0, 0.5);
  }

  h2 {
    margin: 0;
    font-size: 16px;
    font-weight: 700;
  }

  .intro {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
    color: var(--fg-muted);
  }

  .check {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    font-size: 13px;
    line-height: 1.4;
    cursor: pointer;
    padding: 10px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: color-mix(in srgb, var(--bg-card) 88%, #fff 12%);
  }

  .check input {
    margin-top: 2px;
    accent-color: var(--fg);
  }

  .hint {
    margin: 0 0 0 4px;
    font-size: 11px;
    color: var(--fg-subtle);
    line-height: 1.4;
  }

  .opt-out {
    margin: 4px 0 0 0;
    font-size: 11px;
    color: var(--fg-subtle);
    line-height: 1.4;
  }

  .actions {
    margin-top: 8px;
    display: flex;
    justify-content: flex-end;
  }

  .primary {
    border: none;
    border-radius: 8px;
    padding: 10px 18px;
    font-size: 13px;
    font-weight: 700;
    background: var(--fg);
    color: var(--bg-solid);
    cursor: pointer;
    transition: opacity 120ms ease-out;
  }

  .primary:hover:not(:disabled) {
    opacity: 0.85;
  }

  .primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }
</style>
