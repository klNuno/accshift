<script lang="ts">
  import { getSettings, saveSettings, ALL_PLATFORMS } from "./store";

  let { onClose, onPlatformsChanged }: {
    onClose: () => void;
    onPlatformsChanged?: () => void;
  } = $props();

  let settings = $state(getSettings());

  function togglePlatform(id: string) {
    if (settings.enabledPlatforms.includes(id)) {
      if (settings.enabledPlatforms.length <= 1) return;
      settings.enabledPlatforms = settings.enabledPlatforms.filter(p => p !== id);
    } else {
      settings.enabledPlatforms = [...settings.enabledPlatforms, id];
    }
  }

  function save() {
    saveSettings(settings);
    onPlatformsChanged?.();
    onClose();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" onclick={onClose}>
  <div class="panel" onclick={(e) => e.stopPropagation()}>
    <div class="header">
      <span class="title">Settings</span>
      <button class="close-btn" onclick={onClose}>
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
            min="1"
            max="90"
            bind:value={settings.avatarCacheDays}
            class="input"
          />
          <span class="suffix">days</span>
        </div>
        <p class="hint">Cached profile pictures will refresh after this period.</p>
      </div>
    </div>

    <div class="footer">
      <button class="btn-secondary" onclick={onClose}>Cancel</button>
      <button class="btn-primary" onclick={save}>Save</button>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 80;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(4px);
    animation: fadeIn 120ms ease-out;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .panel {
    width: 320px;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.4);
    animation: slideIn 150ms ease-out;
  }

  @keyframes slideIn {
    from { opacity: 0; transform: scale(0.96) translateY(8px); }
    to { opacity: 1; transform: scale(1) translateY(0); }
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
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
    padding: 16px;
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

  .suffix {
    font-size: 12px;
    color: var(--fg-muted);
  }

  .hint {
    font-size: 11px;
    color: var(--fg-subtle);
    margin: 0;
  }

  .footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 16px;
    border-top: 1px solid var(--border);
  }

  .btn-secondary {
    padding: 6px 12px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: transparent;
    color: var(--fg-muted);
    font-size: 12px;
    cursor: pointer;
    transition: all 100ms;
  }

  .btn-secondary:hover {
    background: var(--bg-muted);
    color: var(--fg);
  }

  .btn-primary {
    padding: 6px 12px;
    border: none;
    border-radius: 4px;
    background: var(--fg);
    color: var(--bg);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: all 100ms;
  }

  .btn-primary:hover {
    background: #d4d4d8;
  }

  .btn-primary:active {
    transform: scale(0.97);
  }
</style>
