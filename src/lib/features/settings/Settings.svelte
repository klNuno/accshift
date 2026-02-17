<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getSettings, saveSettings, ALL_PLATFORMS } from "./store";
  import ToggleSetting from "./ToggleSetting.svelte";

  let { onClose, onPlatformsChanged }: {
    onClose: () => void;
    onPlatformsChanged?: () => void;
  } = $props();

  let settings = $state(getSettings());
  let steamEnabled = $derived(settings.enabledPlatforms.includes("steam"));
  let apiKey = $state("");
  let steamPath = $state("");
  let hydrated = $state(false);
  let saveTimer: ReturnType<typeof setTimeout> | null = null;

  function clampInt(value: number, min: number, max: number, fallback: number): number {
    if (!Number.isFinite(value)) return fallback;
    return Math.min(max, Math.max(min, Math.round(value)));
  }

  function normalizeSettings() {
    if (settings.theme !== "light" && settings.theme !== "dark") {
      settings.theme = "dark";
    }
    settings.avatarCacheDays = clampInt(settings.avatarCacheDays, 0, 90, 7);
    settings.banCheckDays = clampInt(settings.banCheckDays, 0, 90, 7);
    settings.inactivityBlurSeconds = clampInt(settings.inactivityBlurSeconds, 0, 3600, 60);
    settings.steamLaunchOptions = (settings.steamLaunchOptions || "").trim();
    settings.pinCode = (settings.pinCode || "").trim();
    if (!ALL_PLATFORMS.some(p => p.id === settings.defaultPlatformId)) {
      settings.defaultPlatformId = "steam";
    }
    if (!settings.enabledPlatforms.length) settings.enabledPlatforms = ["steam"];
    if (!settings.enabledPlatforms.includes(settings.defaultPlatformId)) {
      settings.defaultPlatformId = settings.enabledPlatforms[0];
    }
  }

  async function persistNow() {
    normalizeSettings();
    saveSettings(settings);
    try {
      await invoke("set_api_key", { key: apiKey.trim() });
      await invoke("set_steam_path", { path: steamPath.trim() });
      onPlatformsChanged?.();
    } catch (e) {
      console.error("Failed to save API key:", e);
    }
  }

  function queueSave() {
    if (!hydrated) return;
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      void persistNow();
    }, 220);
  }

  onMount(async () => {
    try {
      apiKey = await invoke<string>("get_api_key");
      steamPath = await invoke<string>("get_steam_path");
    } catch {
      apiKey = "";
      steamPath = "";
    } finally {
      hydrated = true;
    }
  });

  onDestroy(() => {
    if (saveTimer) clearTimeout(saveTimer);
  });

  function togglePlatform(id: string) {
    if (settings.enabledPlatforms.includes(id)) {
      if (settings.enabledPlatforms.length <= 1) return;
      settings.enabledPlatforms = settings.enabledPlatforms.filter(p => p !== id);
    } else {
      settings.enabledPlatforms = [...settings.enabledPlatforms, id];
    }
  }

  $effect(() => {
    settings.avatarCacheDays;
    settings.banCheckDays;
    settings.inactivityBlurSeconds;
    settings.theme;
    settings.steamRunAsAdmin;
    settings.steamLaunchOptions;
    settings.showUsernames;
    settings.showLastLogin;
    settings.defaultPlatformId;
    settings.pinEnabled;
    settings.pinCode;
    settings.enabledPlatforms.join(",");
    apiKey;
    steamPath;
    queueSave();
  });

  async function chooseSteamFolder() {
    try {
      steamPath = await invoke<string>("select_steam_path");
    } catch {
      // user canceled or dialog failed
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="settings-panel">
  <div class="header">
    <div class="title-wrap">
      <span class="title">Settings</span>
    </div>
    <div class="header-actions">
      <button class="close-btn" onclick={onClose} title="Close">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <line x1="18" y1="6" x2="6" y2="18" />
          <line x1="6" y1="6" x2="18" y2="18" />
        </svg>
      </button>
    </div>
  </div>

  <div class="settings-grid">
    <section class="card">
      <h3>Appearance</h3>
      <ToggleSetting
        label="Light mode"
        enabled={settings.theme === "light"}
        accent="#f59e0b"
        onLabel="On"
        offLabel="Off"
        onToggle={() => settings.theme = settings.theme === "light" ? "dark" : "light"}
      />
    </section>

    <section class="card">
      <h3>Platforms</h3>
      <div class="platforms">
        {#each ALL_PLATFORMS as platform}
          <button class="platform-chip" onclick={() => togglePlatform(platform.id)} style={`--chip-accent:${platform.accent};`}>
            <span>{platform.name}</span>
            <div class="toggle" class:active={settings.enabledPlatforms.includes(platform.id)}>
              <div class="knob"></div>
            </div>
          </button>
        {/each}
      </div>

      <div class="field">
        <div class="row">
          <span>Default on startup</span>
          <strong>{ALL_PLATFORMS.find(p => p.id === settings.defaultPlatformId)?.name || settings.defaultPlatformId}</strong>
        </div>
        <select class="text-input" bind:value={settings.defaultPlatformId}>
          {#each ALL_PLATFORMS as platform}
            <option value={platform.id} disabled={!settings.enabledPlatforms.includes(platform.id)}>
              {platform.name}{!settings.enabledPlatforms.includes(platform.id) ? " (disabled)" : ""}
            </option>
          {/each}
        </select>
      </div>
    </section>

    <section class="card">
      <h3>Data Refresh</h3>

      <label class="field">
        <div class="row">
          <span>Avatar refresh</span>
          <span class="hint">0 = each launch</span>
        </div>
        <input
          type="number"
          min="0"
          max="90"
          step="1"
          bind:value={settings.avatarCacheDays}
          class="text-input number-input"
        />
      </label>

      <label class="field">
        <div class="row">
          <span>Ban check delay</span>
          <span class="hint">0 = each launch</span>
        </div>
        <input
          type="number"
          min="0"
          max="90"
          step="1"
          bind:value={settings.banCheckDays}
          class="text-input number-input"
        />
      </label>
    </section>

    <section class="card">
      <h3>Privacy</h3>
      <label class="field">
        <div class="row">
          <span>Inactivity timeout</span>
          <span class="hint">0 = disabled</span>
        </div>
        <input
          type="number"
          min="0"
          max="3600"
          step="5"
          bind:value={settings.inactivityBlurSeconds}
          class="text-input number-input"
        />
      </label>

    </section>

    <section class="card">
      <h3>Security</h3>
      <ToggleSetting
        label="PIN lock on AFK"
        enabled={settings.pinEnabled}
        accent="#eab308"
        onLabel="Enabled"
        offLabel="Disabled"
        onToggle={() => settings.pinEnabled = !settings.pinEnabled}
      />

      {#if settings.pinEnabled}
        <div class="field">
          <div class="row">
            <span>PIN code</span>
            <strong>{settings.pinCode ? "Configured" : "Missing"}</strong>
          </div>
          <input
            id="pin-code"
            type="password"
            bind:value={settings.pinCode}
            class="text-input"
            placeholder="4-8 digits"
            maxlength="16"
          />
        </div>
      {/if}
    </section>

    {#if steamEnabled}
      <section class="card">
        <h3>Steam</h3>

        <ToggleSetting
          label="Run Steam as admin"
          enabled={settings.steamRunAsAdmin}
          onLabel="Enabled"
          offLabel="Disabled"
          onToggle={() => settings.steamRunAsAdmin = !settings.steamRunAsAdmin}
        />

        <ToggleSetting
          label="Show usernames"
          enabled={settings.showUsernames}
          onLabel="Visible"
          offLabel="Hidden"
          onToggle={() => settings.showUsernames = !settings.showUsernames}
        />

        <ToggleSetting
          label="Visible last login"
          enabled={settings.showLastLogin}
          onLabel="On"
          offLabel="Off"
          onToggle={() => settings.showLastLogin = !settings.showLastLogin}
        />

        <div class="field">
          <div class="row">
            <span>Launch options</span>
            <strong>{settings.steamLaunchOptions ? "Custom" : "None"}</strong>
          </div>
          <input
            id="steam-launch-options"
            type="text"
            bind:value={settings.steamLaunchOptions}
            class="text-input"
            placeholder="-silent -vgui"
          />
        </div>

        <div class="field">
          <div class="row">
            <span>Steam folder</span>
            <strong>{steamPath ? "Custom" : "Registry"}</strong>
          </div>
          <div class="input-row">
            <input
              id="steam-folder"
              type="text"
              bind:value={steamPath}
              class="text-input"
              placeholder="C:\Program Files (x86)\Steam"
            />
            <button class="browse-btn" type="button" onclick={chooseSteamFolder}>Choose...</button>
          </div>
        </div>

        <div class="field">
          <div class="row">
            <span>Steam Web API key</span>
            <strong>{apiKey.trim() ? "Configured" : "Missing"}</strong>
          </div>
          <input id="api-key" type="password" bind:value={apiKey} class="text-input" placeholder="Paste your Steam Web API key" />
        </div>
      </section>
    {/if}
  </div>
</div>

<style>
  .settings-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow-y: auto;
    animation: fadeIn 140ms ease-out;
  }

  @keyframes fadeIn {
    from { opacity: 0; transform: translateY(4px); }
    to { opacity: 1; transform: translateY(0); }
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding-bottom: 10px;
    border-bottom: 1px solid var(--border);
  }

  .title-wrap {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .title {
    font-size: 14px;
    font-weight: 700;
    color: var(--fg);
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .close-btn {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
  }

  .close-btn:hover {
    background: var(--bg-muted);
    color: var(--fg);
  }

  .settings-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: 10px;
    padding-bottom: 8px;
  }

  .card {
    background: color-mix(in srgb, var(--bg-card) 84%, #000 16%);
    border: 1px solid color-mix(in srgb, var(--border) 80%, #fff 20%);
    border-radius: 10px;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .card h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 650;
    color: var(--fg);
  }

  .platforms {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .platform-chip {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: color-mix(in srgb, var(--bg) 88%, #fff 12%);
    color: var(--fg);
    padding: 9px 10px;
    cursor: pointer;
    transition: border-color 120ms ease-out, background 120ms ease-out;
  }

  .platform-chip:hover {
    border-color: color-mix(in srgb, var(--chip-accent) 55%, var(--border));
    background: color-mix(in srgb, var(--bg-card) 84%, #fff 16%);
  }

  .toggle {
    width: 36px;
    height: 20px;
    border-radius: 999px;
    background: var(--bg-elevated);
    padding: 2px;
    transition: background 120ms ease-out;
  }

  .toggle.active {
    background: var(--chip-accent);
  }

  .knob {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    transition: transform 120ms ease-out;
  }

  .toggle.active .knob {
    transform: translateX(16px);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    font-size: 12px;
    color: var(--fg-muted);
  }

  .row strong {
    font-size: 12px;
    color: var(--fg);
    font-weight: 600;
  }

  .hint {
    font-size: 11px;
    color: var(--fg-subtle);
  }

  .text-input {
    width: 100%;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    color: var(--fg);
    font-size: 12px;
    padding: 9px 10px;
    outline: none;
  }

  .text-input:focus {
    border-color: #3b82f6;
  }

  .input-row {
    display: flex;
    gap: 8px;
  }

  .number-input {
    width: 100%;
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

  .browse-btn:hover {
    background: var(--bg-card-hover);
  }
</style>
