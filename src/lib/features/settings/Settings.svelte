<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getSettings, saveSettings, ALL_PLATFORMS } from "./store";

  let { onClose, onPlatformsChanged }: {
    onClose: () => void;
    onPlatformsChanged?: () => void;
  } = $props();

  let settings = $state(getSettings());
  let apiKey = $state("");

  onMount(async () => {
    try {
      apiKey = await invoke<string>("get_api_key");
    } catch {
      apiKey = "";
    }
  });

  function togglePlatform(id: string) {
    if (settings.enabledPlatforms.includes(id)) {
      if (settings.enabledPlatforms.length <= 1) return;
      settings.enabledPlatforms = settings.enabledPlatforms.filter(p => p !== id);
    } else {
      settings.enabledPlatforms = [...settings.enabledPlatforms, id];
    }
  }

  async function save() {
    saveSettings(settings);
    try {
      await invoke("set_api_key", { key: apiKey });
    } catch (e) {
      console.error("Failed to save API key:", e);
    }
    onPlatformsChanged?.();
  }

  $effect(() => {
    // Auto-save whenever settings or apiKey changes
    save();
  });

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="settings-panel">
  <div class="header">
    <span class="title">Settings</span>
    <button class="close-btn" onclick={onClose} title="Close">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <line x1="18" y1="6" x2="6" y2="18" />
        <line x1="6" y1="6" x2="18" y2="18" />
      </svg>
    </button>
  </div>

  <div class="body">
    <div class="section-label">Platforms</div>
    <div class="platforms">
      {#each ALL_PLATFORMS as platform}
        <button
          class="platform-row"
          onclick={() => togglePlatform(platform.id)}
        >
          <span class="platform-name">{platform.name}</span>
          <div
            class="toggle"
            class:active={settings.enabledPlatforms.includes(platform.id)}
            style={settings.enabledPlatforms.includes(platform.id) ? `background: ${platform.accent};` : ""}
          >
            <div class="toggle-knob"></div>
          </div>
        </button>
      {/each}
    </div>

    <div class="divider"></div>

    <div class="field">
      <label class="label" for="cache-days">Avatar refresh delay</label>
      <div class="input-row">
        <input
          id="cache-days"
          type="number"
          min="0"
          max="90"
          bind:value={settings.avatarCacheDays}
          class="input"
        />
        <span class="suffix">days</span>
      </div>
      <p class="hint">Cached profile pictures will refresh after this period. 0 = refresh on every launch.</p>
    </div>

    <div class="divider"></div>

    <div class="field">
      <label class="label" for="ban-check-days">Ban check delay</label>
      <div class="input-row">
        <input
          id="ban-check-days"
          type="number"
          min="0"
          max="90"
          bind:value={settings.banCheckDays}
          class="input"
        />
        <span class="suffix">days</span>
      </div>
      <p class="hint">Check game bans every X days. 0 = check on every launch.</p>
    </div>

    <div class="divider"></div>

    <div class="field">
      <label class="label" for="blur-seconds">Inactivity blur</label>
      <div class="input-row">
        <input
          id="blur-seconds"
          type="number"
          min="0"
          max="3600"
          bind:value={settings.inactivityBlurSeconds}
          class="input"
        />
        <span class="suffix">seconds</span>
      </div>
      <p class="hint">Blur app content after inactivity. 0 to disable.</p>
    </div>

    <div class="divider"></div>

    <div class="field">
      <label class="label" for="api-key">Steam API Key</label>
      <div class="input-row">
        <input
          id="api-key"
          type="password"
          bind:value={apiKey}
          class="input input-wide"
          placeholder="Optional"
        />
      </div>
      <p class="hint">Optional. Enables community & game ban detection.</p>
    </div>
  </div>
</div>

<style>
  .settings-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    animation: fadeIn 120ms ease-out;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-bottom: 12px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 12px;
  }

  .title {
    font-size: 13px;
    font-weight: 600;
    color: var(--fg);
  }

  .close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
    transition: all 100ms;
  }

  .close-btn:hover {
    background: var(--bg-muted);
    color: var(--fg);
  }

  .body {
    flex: 1;
  }

  .section-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--fg-subtle);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 8px;
  }

  .platforms {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .platform-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 10px;
    border: none;
    border-radius: 6px;
    background: transparent;
    cursor: pointer;
    transition: background 100ms;
  }

  .platform-row:hover {
    background: var(--bg-muted);
  }

  .platform-name {
    font-size: 13px;
    color: var(--fg);
  }

  .toggle {
    width: 34px;
    height: 18px;
    border-radius: 9px;
    background: var(--bg-elevated);
    position: relative;
    transition: background 150ms;
  }

  .toggle.active .toggle-knob {
    transform: translateX(16px);
  }

  .toggle-knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--fg);
    transition: transform 150ms;
  }

  .divider {
    height: 1px;
    background: var(--border);
    margin: 16px 0;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .label {
    font-size: 12px;
    font-weight: 500;
    color: var(--fg);
  }

  .input-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .input {
    width: 64px;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg);
    color: var(--fg);
    font-size: 13px;
    outline: none;
    transition: border-color 120ms;
  }

  .input:focus {
    border-color: var(--bg-elevated);
  }

  .input-wide {
    width: 100%;
    flex: 1;
  }

  .suffix {
    font-size: 12px;
    color: var(--fg-muted);
  }

  .hint {
    font-size: 11px;
    color: var(--fg-subtle);
    margin: 0;
  }
</style>
