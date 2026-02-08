<script lang="ts">
  import { getSettings, saveSettings } from "$lib/settings";

  let { onClose }: { onClose: () => void } = $props();

  let settings = $state(getSettings());

  function save() {
    saveSettings(settings);
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
    background: #1c1c1f;
    border: 1px solid #27272a;
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
    border-bottom: 1px solid #27272a;
  }

  .title {
    font-size: 13px;
    font-weight: 600;
    color: #fafafa;
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
    color: #a1a1aa;
    cursor: pointer;
    transition: all 100ms;
  }

  .close-btn:hover {
    background: #27272a;
    color: #fafafa;
  }

  .body {
    padding: 16px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .label {
    font-size: 12px;
    font-weight: 500;
    color: #fafafa;
  }

  .input-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .input {
    width: 64px;
    padding: 6px 8px;
    border: 1px solid #27272a;
    border-radius: 4px;
    background: #09090b;
    color: #fafafa;
    font-size: 13px;
    outline: none;
    transition: border-color 120ms;
  }

  .input:focus {
    border-color: #3f3f46;
  }

  .suffix {
    font-size: 12px;
    color: #a1a1aa;
  }

  .hint {
    font-size: 11px;
    color: #71717a;
    margin: 0;
  }

  .footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 16px;
    border-top: 1px solid #27272a;
  }

  .btn-secondary {
    padding: 6px 12px;
    border: 1px solid #27272a;
    border-radius: 4px;
    background: transparent;
    color: #a1a1aa;
    font-size: 12px;
    cursor: pointer;
    transition: all 100ms;
  }

  .btn-secondary:hover {
    background: #27272a;
    color: #fafafa;
  }

  .btn-primary {
    padding: 6px 12px;
    border: none;
    border-radius: 4px;
    background: #fafafa;
    color: #09090b;
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
