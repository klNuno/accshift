<script lang="ts">
  import ToggleSetting from "$lib/features/settings/ToggleSetting.svelte";
  import type { AppSettings } from "$lib/features/settings/types";
  import type { MessageKey, TranslationParams } from "$lib/i18n";
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
  let statusMessage = $state("");
  let statusError = $state(false);

  async function handleAddByCookie() {
    const trimmed = cookieValue.trim();
    if (!trimmed || isAdding) return;
    isAdding = true;
    statusMessage = "";
    statusError = false;
    try {
      const account = await addAccountByCookie(trimmed);
      cookieValue = "";
      statusMessage = t("roblox.setupReadyWithProfile", { profile: account.displayName || account.username });
      statusError = false;
    } catch (err) {
      statusMessage = String(err);
      statusError = true;
    } finally {
      isAdding = false;
    }
  }
</script>

<section class="card platform-display-card" style={`--display-accent:${accent};`}>
  <h3>{t("settings.accountDisplay")}</h3>
  <ToggleSetting
    label={t("settings.showRobloxLastLogin")}
    enabled={settings.accountDisplay.showLastLoginPerPlatform["roblox"] ?? true}
    {accent}
    onLabel={t("common.on")}
    offLabel={t("common.off")}
    onToggle={() => settings.accountDisplay.showLastLoginPerPlatform["roblox"] = !settings.accountDisplay.showLastLoginPerPlatform["roblox"]}
  />
</section>

<section class="card platform-display-card" style={`--display-accent:${accent};`}>
  <h3>{t("roblox.cookiePasteTitle")}</h3>
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
  {#if statusMessage}
    <p class="status" class:error={statusError}>{statusMessage}</p>
  {/if}
</section>

<style>
  .card {
    background: color-mix(in srgb, var(--bg-card) 84%, #000 16%);
    border: 1px solid color-mix(in srgb, var(--border) 80%, #fff 20%);
    border-radius: 12px;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .card h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 700;
    color: var(--fg);
  }

  .platform-display-card {
    border-color: color-mix(in srgb, var(--display-accent) 32%, var(--border));
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--display-accent) 12%, transparent);
  }

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

  .status {
    margin: 0;
    font-size: 11px;
    color: #86efac;
  }

  .status.error {
    color: #fca5a5;
  }
</style>
