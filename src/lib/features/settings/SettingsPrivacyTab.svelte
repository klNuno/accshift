<script lang="ts">
  import { sanitizePinDigits } from "$lib/shared/pin";
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
</script>

<div class="settings-grid">
  <section class="card">
    <h3>{t("settings.privacy")}</h3>
    <label class="field">
      <span class="field-label">{t("settings.inactivityTimeout")} <span class="hint">({t("settings.zeroDisabled")})</span></span>
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
      </div>
    {/if}
  </section>
</div>
