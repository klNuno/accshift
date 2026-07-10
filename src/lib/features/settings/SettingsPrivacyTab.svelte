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
    t,
    inactivityBlur,
    neutralAccent,
  }: {
    settings: AppSettings;
    pinCodeInput: string;
    t: (key: MessageKey, params?: TranslationParams) => string;
    inactivityBlur: { input: string; commit: () => void };
    neutralAccent: string;
  } = $props();

  type TelemetryState = {
    mode_a_enabled: boolean;
    mode_b_enabled: boolean;
    install_id_set: boolean;
    onboarding_completed: boolean;
  };

  const NOTE_MAX_CHARS = 1000;

  let telemetry = $state<TelemetryState | null>(null);
  let telemetryError = $state(false);
  let modeBBusy = $state(false);
  let exportBusy = $state(false);
  let sendLogsBusy = $state(false);
  let logsNote = $state("");

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
      addToast(t("settings.telemetryDisableFailed"));
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

  async function sendLogs() {
    if (sendLogsBusy) return;
    sendLogsBusy = true;
    try {
      const trimmed = logsNote.trim();
      const note = trimmed.length > 0 ? trimmed.slice(0, NOTE_MAX_CHARS) : null;
      const ticketId = await invoke<string>("telemetry_upload_logs", { note });
      try {
        await navigator.clipboard.writeText(ticketId);
      } catch {
        // Ignore clipboard errors. The ticket id is still shown in the toast.
      }
      addToast(t("settings.sendLogsSuccess", { id: ticketId }));
      logsNote = "";
    } catch (e) {
      console.error("telemetry_upload_logs failed", e);
      addToast(t("settings.sendLogsFailed"));
    } finally {
      sendLogsBusy = false;
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
      enabled={settings.streamerMode === "auto"}
      accent={neutralAccent}
      onLabel={t("common.enabled")}
      offLabel={t("common.disabled")}
      onToggle={() => {
        settings.streamerMode = settings.streamerMode === "auto" ? "off" : "auto";
      }}
    />
    <p class="hint">{t("settings.streamerModeHint")}</p>
  </section>

  <section class="card">
    <h3>{t("settings.security")}</h3>
    <ToggleSetting
      label={t("settings.pinLockOnAfk")}
      enabled={settings.pinEnabled}
      accent={neutralAccent}
      onLabel={t("common.enabled")}
      offLabel={t("common.disabled")}
      onToggle={() => {
        settings.pinEnabled = !settings.pinEnabled;
        if (!settings.pinEnabled) {
          settings.pinHash = "";
          pinCodeInput = "";
        }
      }}
    />

    {#if settings.pinEnabled}
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
        <p class="hint">{t("settings.pinTakesEffectNextLaunch")}</p>
      </div>
    {/if}
  </section>

  {#if telemetry}
    <section class="card card-wide">
      <h3>{t("settings.telemetry")}</h3>

      <ToggleSetting
        label={t("settings.telemetryModeA")}
        enabled={telemetry.mode_a_enabled}
        accent={neutralAccent}
        onLabel={t("common.enabled")}
        offLabel={t("common.disabled")}
        onToggle={toggleModeA}
      />
      <p class="hint">{t("settings.telemetryModeAHint")}</p>

      <ToggleSetting
        label={t("settings.telemetryModeB")}
        enabled={telemetry.mode_b_enabled}
        accent={neutralAccent}
        onLabel={t("common.enabled")}
        offLabel={t("common.disabled")}
        disabled={modeBBusy}
        onToggle={toggleModeB}
      />
      <p class="hint">{t("settings.telemetryModeBHint")}</p>

      {#if telemetry.mode_b_enabled && telemetry.install_id_set}
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

    <section class="card card-wide">
      <h3>{t("settings.sendLogs")}</h3>
      <p class="hint">{t("settings.sendLogsHint")}</p>
      <label class="note-field">
        <span class="note-label">{t("settings.sendLogsNoteLabel")}</span>
        <textarea
          class="note-input"
          rows="3"
          maxlength={NOTE_MAX_CHARS}
          placeholder={t("settings.sendLogsNotePlaceholder")}
          bind:value={logsNote}
        ></textarea>
        <span class="note-counter">{logsNote.length}/{NOTE_MAX_CHARS}</span>
      </label>
      <button
        type="button"
        class="btn-export"
        disabled={sendLogsBusy}
        onclick={sendLogs}
      >
        {t("settings.sendLogs")}
      </button>
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

  .note-field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 8px;
  }

  .note-label {
    font-size: 11px;
    color: var(--fg-subtle);
    line-height: 1.4;
  }

  .note-input {
    width: 100%;
    box-sizing: border-box;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-card);
    color: var(--fg);
    padding: 8px 10px;
    font-size: 12px;
    font-family: inherit;
    line-height: 1.5;
    resize: vertical;
    min-height: 60px;
    transition: border-color 120ms ease-out;
  }

  .note-input:focus {
    outline: none;
    border-color: color-mix(in srgb, var(--fg) 35%, var(--border));
  }

  .note-counter {
    align-self: flex-end;
    font-size: 10px;
    color: var(--fg-subtle);
  }
</style>
