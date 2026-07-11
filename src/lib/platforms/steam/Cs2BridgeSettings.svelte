<script lang="ts">
  import ToggleSetting from "$lib/features/settings/ToggleSetting.svelte";
  import { addToast } from "$lib/features/notifications/store.svelte";
  import { getCs2BridgeSettings, setCs2BridgeSettings, testCs2Bridge } from "./steamApi";
  import { invalidateCs2Bridge, loadCs2BridgeData } from "./cs2Bridge.svelte";
  import type { MessageKey, TranslationParams } from "$lib/i18n";

  let {
    accent = "#2563eb",
    t,
  }: {
    accent?: string;
    t: (key: MessageKey, params?: TranslationParams) => string;
  } = $props();

  let enabled = $state(false);
  let url = $state("");
  let tokenInput = $state("");
  let tokenConfigured = $state(false);
  let loaded = $state(false);
  let urlError = $state(false);

  type Status = "unknown" | "testing" | "ok" | "fail";
  let status = $state<Status>("unknown");
  let statusDetail = $state("");
  let statusError = $state("");

  $effect(() => {
    void (async () => {
      try {
        const settings = await getCs2BridgeSettings();
        enabled = settings.enabled;
        url = settings.url;
        tokenConfigured = settings.tokenConfigured;
        if (settings.enabled && settings.url) void runTest();
      } catch {
        // Backend unavailable: leave the defaults, saving will surface errors.
      } finally {
        loaded = true;
      }
    })();
  });

  async function runTest() {
    if (!url.trim()) {
      status = "unknown";
      statusDetail = "";
      statusError = "";
      return;
    }
    status = "testing";
    statusError = "";
    try {
      const result = await testCs2Bridge();
      if (result.ok) {
        status = "ok";
        statusDetail = t("settings.cs2BridgeStatusOk", {
          count: result.accountCount,
          ms: result.latencyMs,
        });
        statusError = "";
      } else {
        status = "fail";
        statusDetail = t("settings.cs2BridgeStatusFail");
        statusError = result.error ?? "";
      }
    } catch (error) {
      status = "fail";
      statusDetail = t("settings.cs2BridgeStatusFail");
      statusError = String(error);
    }
  }

  async function save(token: string | null) {
    try {
      await setCs2BridgeSettings(enabled, url.trim(), token);
      urlError = false;
      if (token !== null) {
        tokenConfigured = token.trim().length > 0;
        tokenInput = "";
      }
      invalidateCs2Bridge();
      if (enabled) void loadCs2BridgeData(true);
      void runTest();
    } catch {
      urlError = true;
      addToast(t("settings.cs2BridgeSaveFailed"), { type: "error" });
    }
  }

  async function toggleEnabled() {
    enabled = !enabled;
    await save(null);
  }
</script>

<section class="card platform-display-card" style={`--display-accent:${accent};`}>
  <div class="title-row">
    <h3>{t("settings.cs2Bridge")}</h3>
    <div class="status" title={statusError || statusDetail}>
      <span class={`status-dot ${status}`} aria-hidden="true"></span>
      <span class="status-text">
        {#if status === "testing"}
          {t("settings.cs2BridgeStatusTesting")}
        {:else if status === "unknown"}
          {t("settings.cs2BridgeStatusUnknown")}
        {:else}
          {statusDetail}
        {/if}
      </span>
    </div>
  </div>
  <p class="hint-text">{t("settings.cs2BridgeHint")}</p>

  <ToggleSetting
    label={t("settings.cs2Bridge")}
    enabled={enabled}
    accent={accent}
    onLabel={t("common.enabled")}
    offLabel={t("common.disabled")}
    onToggle={() => { if (loaded) void toggleEnabled(); }}
  />

  <label class="field">
    <span class="field-label">{t("settings.cs2BridgeLink")}</span>
    <div class="input-row">
      <input
        type="text"
        bind:value={url}
        class="text-input"
        class:invalid={urlError}
        placeholder={t("settings.cs2BridgeLinkPlaceholder")}
        onblur={() => { if (loaded) void save(null); }}
      />
      <button class="browse-btn" type="button" onclick={() => void runTest()} disabled={status === "testing"}>
        {t("settings.cs2BridgeTest")}
      </button>
    </div>
    {#if status === "fail" && statusError}
      <p class="error-hint">{statusError}</p>
    {/if}
  </label>

  <div class="field">
    <span class="field-label">
      {t("settings.cs2BridgeToken")}
      <span class="hint">({t("settings.cs2BridgeTokenOptional")})</span>
      {#if tokenConfigured}
        <span class="key-badge">{t("common.configured")}</span>
      {/if}
    </span>
    <div class="input-row">
      <input
        type="password"
        bind:value={tokenInput}
        class="text-input"
        placeholder={t("settings.cs2BridgeTokenPlaceholder")}
        onblur={() => { if (loaded && tokenInput.trim()) void save(tokenInput); }}
      />
      {#if tokenConfigured}
        <button class="clear-key-btn" type="button" onclick={() => void save("")}>
          {t("settings.cs2BridgeClearToken")}
        </button>
      {/if}
    </div>
  </div>
</section>

<style>
  .card {
    background: color-mix(in srgb, var(--bg-card) 84%, #000 16%);
    border: 1px solid color-mix(in srgb, var(--border) 80%, #fff 20%);
    border-radius: 10px;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .title-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding-bottom: 8px;
    border-bottom: 1px solid color-mix(in srgb, var(--border) 65%, transparent);
  }

  .card h3 {
    margin: 0;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--fg-muted);
  }

  .platform-display-card {
    border-color: color-mix(in srgb, var(--display-accent) 32%, var(--border));
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--display-accent) 12%, transparent);
  }

  .status {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .status-dot {
    flex: 0 0 auto;
    width: 8px;
    height: 8px;
    border-radius: 999px;
    background: var(--fg-subtle);
  }

  .status-dot.ok {
    background: #22c55e;
    box-shadow: 0 0 6px rgba(34, 197, 94, 0.55);
  }

  .status-dot.fail {
    background: #ef4444;
    box-shadow: 0 0 6px rgba(239, 68, 68, 0.55);
  }

  .status-dot.testing {
    background: #eab308;
  }

  .status-text {
    font-size: 10px;
    color: var(--fg-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .hint-text {
    margin: 0;
    font-size: 11px;
    color: var(--fg-subtle);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .field-label {
    font-size: 12px;
    color: var(--fg-muted);
  }

  .hint {
    font-size: 11px;
    color: var(--fg-subtle);
  }

  .input-row {
    display: flex;
    gap: 8px;
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

  .text-input.invalid,
  .text-input.invalid:focus {
    border-color: #ef4444;
  }

  .error-hint {
    margin: 0;
    font-size: 11px;
    color: #fca5a5;
    word-break: break-word;
  }

  .key-badge {
    display: inline-block;
    margin-left: 6px;
    padding: 1px 7px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, #22c55e 45%, var(--border));
    background: color-mix(in srgb, #22c55e 14%, transparent);
    color: #4ade80;
    font-size: 10px;
    font-weight: 600;
    vertical-align: middle;
  }

  .clear-key-btn {
    flex-shrink: 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-card);
    color: var(--fg-muted);
    font-size: 11px;
    padding: 0 10px;
    cursor: pointer;
    white-space: nowrap;
    transition: border-color 120ms ease-out, color 120ms ease-out;
  }

  .clear-key-btn:hover {
    color: #fca5a5;
    border-color: color-mix(in srgb, #ef4444 45%, var(--border));
  }

  .browse-btn {
    flex-shrink: 0;
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

  .browse-btn:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
