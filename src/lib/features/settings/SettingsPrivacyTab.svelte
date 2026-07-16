<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { sanitizePinDigits } from "$lib/shared/pin";
  import { addToast } from "../notifications/store.svelte";
  import ToggleSetting from "./ToggleSetting.svelte";
  import type { MessageKey, TranslationParams } from "$lib/i18n";
  import type { AppSettings } from "./types";

  const PIN_CODE_LENGTH = 4;

  let {
    settings = $bindable(),
    pinCodeInput = $bindable(),
    pinSetupPending = $bindable(),
    t,
    inactivityBlur,
    neutralAccent,
    onReplayOnboarding = () => {},
  }: {
    settings: AppSettings;
    pinCodeInput: string;
    pinSetupPending: boolean;
    t: (key: MessageKey, params?: TranslationParams) => string;
    inactivityBlur: { input: string; commit: () => void };
    neutralAccent: string;
    onReplayOnboarding?: () => void | Promise<void>;
  } = $props();

  type TelemetryState = {
    mode_a_enabled: boolean;
    mode_b_enabled: boolean;
    install_id_set: boolean;
    forget_pending: boolean;
    onboarding_completed: boolean;
  };

  let telemetry = $state<TelemetryState | null>(null);
  let telemetryError = $state(false);
  let modeBBusy = $state(false);
  let exportBusy = $state(false);

  async function refreshTelemetry() {
    try {
      telemetry = await invoke<TelemetryState>("telemetry_get_state");
      telemetryError = false;
    } catch (e) {
      console.error("telemetry_get_state failed", e);
      telemetryError = true;
    }
  }

  onMount(refreshTelemetry);

  async function toggleModeA() {
    if (!telemetry) return;
    const next = !telemetry.mode_a_enabled;
    telemetry = { ...telemetry, mode_a_enabled: next };
    try {
      await invoke("telemetry_set_mode_a", { enabled: next });
    } catch (e) {
      console.error("telemetry_set_mode_a failed", e);
      await refreshTelemetry();
    }
  }

  async function toggleModeB() {
    if (!telemetry || modeBBusy) return;
    const next = !telemetry.mode_b_enabled;
    modeBBusy = true;
    try {
      await invoke("telemetry_set_mode_b", { enabled: next });
      await refreshTelemetry();
    } catch (e) {
      console.error("telemetry_set_mode_b failed", e);
      await refreshTelemetry();
      const failureKey = next
        ? "settings.telemetryEnableFailed"
        : telemetry && !telemetry.mode_b_enabled
          ? "settings.telemetryDeletePending"
          : "settings.telemetryDisableFailed";
      addToast(t(failureKey));
    } finally {
      modeBBusy = false;
    }
  }

  async function retryTelemetryDeletion() {
    if (modeBBusy) return;
    modeBBusy = true;
    try {
      await invoke("telemetry_retry_forget");
      await refreshTelemetry();
      addToast(t("settings.telemetryDeleteCompleted"));
    } catch (e) {
      console.error("telemetry_retry_forget failed", e);
      await refreshTelemetry();
      addToast(t("settings.telemetryDeleteRetryFailed"));
    } finally {
      modeBBusy = false;
    }
  }

  async function exportMyData() {
    if (exportBusy) return;
    exportBusy = true;
    try {
      const data = await invoke<unknown>("telemetry_export");
      await navigator.clipboard.writeText(JSON.stringify(data, null, 2));
      addToast(t("settings.telemetryExported"));
    } catch (e) {
      console.error("telemetry_export failed", e);
      addToast(t("settings.telemetryExportFailed"));
    } finally {
      exportBusy = false;
    }
  }

</script>

<div class="settings-grid">
  <section class="card">
    <h3>{t("settings.privacy")}</h3>
    <label class="field">
      <span class="field-label">{t("settings.inactivityTimeout")} <span class="hint">({t("settings.unitSeconds")}, {t("settings.zeroDisabled")})</span></span>
      <input
        type="number"
        min="0"
        max="3600"
        step="5"
        value={inactivityBlur.input}
        oninput={(e) => inactivityBlur.input = (e.currentTarget as HTMLInputElement).value}
        onblur={inactivityBlur.commit}
        onkeydown={(e) => {
          if (e.key === "Enter") {
            inactivityBlur.commit();
            (e.currentTarget as HTMLInputElement).blur();
          }
        }}
        class="text-input number-input"
      />
    </label>
    <ToggleSetting
      label={t("settings.streamerMode")}
      description={t("settings.streamerModeHint")}
      enabled={settings.streamerMode === "auto"}
      accent={neutralAccent}
      onLabel={t("common.enabled")}
      offLabel={t("common.disabled")}
      onToggle={() => {
        settings.streamerMode = settings.streamerMode === "auto" ? "off" : "auto";
      }}
    />
    <button type="button" class="btn-export" onclick={() => void onReplayOnboarding()}>
      {t("settings.replayOnboarding")}
    </button>
    <p class="hint">{t("settings.replayOnboardingHint")}</p>
  </section>

  <section class="card">
    <h3>{t("settings.security")}</h3>
    <ToggleSetting
      label={t("settings.pinLockOnAfk")}
      enabled={settings.pinEnabled || pinSetupPending}
      accent={neutralAccent}
      onLabel={t("common.enabled")}
      offLabel={t("common.disabled")}
      onToggle={() => {
        if (settings.pinEnabled || pinSetupPending) {
          settings.pinEnabled = false;
          pinSetupPending = false;
          settings.pinHash = "";
          pinCodeInput = "";
        } else {
          pinSetupPending = true;
        }
      }}
    />

    {#if settings.pinEnabled || pinSetupPending}
      <div class="field">
        <span class="field-label">{t("settings.pinCode")}</span>
        <input
          id="pin-code"
          type="password"
          bind:value={pinCodeInput}
          class="text-input"
          placeholder={t("settings.pinPlaceholder")}
          maxlength={PIN_CODE_LENGTH}
          inputmode="numeric"
          pattern="[0-9]*"
          oninput={(e) => pinCodeInput = sanitizePinDigits((e.currentTarget as HTMLInputElement).value)}
        />
        <p class="hint">{t("settings.pinRequiredAfterInactivity")}</p>
      </div>
    {/if}
  </section>

  {#if telemetry}
    <section class="card card-wide">
      <h3>{t("settings.telemetry")}</h3>

      <ToggleSetting
        label={t("settings.telemetryModeA")}
        description={t("settings.telemetryModeAHint")}
        enabled={telemetry.mode_a_enabled}
        accent={neutralAccent}
        onLabel={t("common.enabled")}
        offLabel={t("common.disabled")}
        onToggle={toggleModeA}
      />

      <ToggleSetting
        label={t("settings.telemetryModeB")}
        description={t("settings.telemetryModeBHint")}
        enabled={telemetry.mode_b_enabled}
        accent={neutralAccent}
        onLabel={t("common.enabled")}
        offLabel={t("common.disabled")}
        disabled={modeBBusy}
        onToggle={toggleModeB}
      />

      {#if telemetry.forget_pending}
        <p class="hint">{t("settings.telemetryDeletePendingHint")}</p>
        <button
          type="button"
          class="btn-export"
          disabled={modeBBusy}
          onclick={retryTelemetryDeletion}
        >
          {t("settings.telemetryRetryDelete")}
        </button>
      {/if}

      {#if telemetry.install_id_set}
        <button
          type="button"
          class="btn-export"
          disabled={exportBusy}
          onclick={exportMyData}
        >
          {t("settings.telemetryExport")}
        </button>
      {/if}
    </section>
  {:else if telemetryError}
    <section class="card card-wide">
      <h3>{t("settings.telemetry")}</h3>
      <p class="hint">{t("settings.telemetryLoadFailed")}</p>
      <button type="button" class="btn-export" onclick={refreshTelemetry}>
        {t("common.retry")}
      </button>
    </section>
  {/if}
</div>

<style>
  .hint {
    margin: 0;
    font-size: 11px;
    color: var(--fg-subtle);
    line-height: 1.4;
  }

  .btn-export {
    align-self: flex-start;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: color-mix(in srgb, var(--bg-card) 88%, #fff 12%);
    color: var(--fg);
    padding: 8px 14px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: border-color 120ms ease-out, background 120ms ease-out;
  }

  .btn-export:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--fg) 35%, var(--border));
    background: color-mix(in srgb, var(--bg-card) 82%, #fff 18%);
  }

  .btn-export:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
