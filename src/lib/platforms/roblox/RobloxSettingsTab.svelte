<script lang="ts">
  import ToggleSetting from "$lib/features/settings/ToggleSetting.svelte";
  import type { AppSettings } from "$lib/features/settings/types";
  import type { MessageKey, TranslationParams } from "$lib/i18n";
  import SettingsCard from "$lib/shared/components/SettingsCard.svelte";
  import { addToast } from "$lib/features/notifications/store.svelte";
  import { addAccountByCookie } from "./robloxApi";

  let {
    settings = $bindable(),
    accent,
    t,
  }: {
    settings: AppSettings;
    accent: string;
    t: (key: MessageKey, params?: TranslationParams) => string;
  } = $props();

  let cookieValue = $state("");
  let isAdding = $state(false);
  let errorMessage = $state("");

  async function handleAddByCookie() {
    const trimmed = cookieValue.trim();
    if (!trimmed || isAdding) return;
    isAdding = true;
    errorMessage = "";
    try {
      const account = await addAccountByCookie(trimmed);
      cookieValue = "";
      addToast(t("roblox.setupReadyWithProfile", { profile: account.displayName || account.username }));
    } catch (err) {
      errorMessage = String(err);
    } finally {
      isAdding = false;
    }
  }
</script>

<SettingsCard title={t("settings.accountDisplay")} {accent}>
  <ToggleSetting
    label={t("settings.showRobloxLastLogin")}
    enabled={settings.accountDisplay.showLastLoginPerPlatform["roblox"] ?? true}
    {accent}
    onLabel={t("common.on")}
    offLabel={t("common.off")}
    onToggle={() => settings.accountDisplay.showLastLoginPerPlatform["roblox"] = !settings.accountDisplay.showLastLoginPerPlatform["roblox"]}
  />
</SettingsCard>

<SettingsCard title={t("roblox.cookiePasteTitle")} {accent}>
  <p class="hint">{t("roblox.cookiePasteHint")}</p>
  <div class="input-row">
    <input
      type="password"
      bind:value={cookieValue}
      class="text-input"
      placeholder=".ROBLOSECURITY"
      disabled={isAdding}
    />
    <button
      class="browse-btn"
      type="button"
      onclick={handleAddByCookie}
      disabled={isAdding || !cookieValue.trim()}
    >{isAdding ? "..." : t("common.add")}</button>
  </div>
  {#if errorMessage}
    <p class="status error">{errorMessage}</p>
  {/if}
</SettingsCard>

<style>
  .hint {
    margin: 0;
    font-size: 11px;
    color: var(--fg-muted);
    line-height: 1.4;
  }

  .text-input {
    width: 100%;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-solid);
    color: var(--fg);
    font-size: 12px;
    padding: 9px 10px;
    outline: none;
  }

  .text-input:focus {
    border-color: color-mix(in srgb, var(--fg-muted) 55%, var(--border));
  }

  .input-row {
    display: flex;
    gap: 8px;
  }

  .browse-btn {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-card);
    color: var(--fg);
    font-size: 12px;
    padding: 0 12px;
    cursor: pointer;
    white-space: nowrap;
  }

  .browse-btn:hover:not(:disabled) {
    background: var(--bg-card-hover);
  }

  .browse-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .status.error {
    margin: 0;
    font-size: 11px;
    color: #fca5a5;
  }
</style>
