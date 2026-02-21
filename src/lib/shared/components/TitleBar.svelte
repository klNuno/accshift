<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type { PlatformDef } from "../../features/settings/types";
  import { DEFAULT_LOCALE, translate, type Locale } from "$lib/i18n";

  let {
    onRefresh,
    onAddAccount,
    onOpenSettings,
    onApplyUpdate = () => {},
    activeTab = "steam",
    onTabChange,
    enabledPlatforms,
    updateCtaLabel = null,
    updateCtaTitle = "Restart to apply update",
    updateCtaDisabled = false,
    locale = DEFAULT_LOCALE,
  }: {
    onRefresh: () => void;
    onAddAccount: () => void;
    onOpenSettings: () => void;
    onApplyUpdate?: () => void;
    activeTab: string;
    onTabChange: (tab: string) => void;
    enabledPlatforms: PlatformDef[];
    updateCtaLabel?: string | null;
    updateCtaTitle?: string;
    updateCtaDisabled?: boolean;
    locale?: Locale;
  } = $props();
  const LOGO_PATHS: Record<string, string> = {
    steam: "M11.979 0C5.678 0 .511 4.86.022 11.037l6.432 2.658c.545-.371 1.203-.59 1.912-.59.063 0 .125.004.188.006l2.861-4.142V8.91c0-2.495 2.028-4.524 4.524-4.524 2.494 0 4.524 2.031 4.524 4.527s-2.03 4.525-4.524 4.525h-.105l-4.076 2.911c0 .052.004.105.004.159 0 1.875-1.515 3.396-3.39 3.396-1.635 0-3.016-1.173-3.331-2.727L.436 15.27C1.862 20.307 6.486 24 11.979 24c6.627 0 11.999-5.373 11.999-12S18.605 0 11.979 0zM7.54 18.21l-1.473-.61c.262.543.714.999 1.314 1.25 1.297.539 2.793-.076 3.332-1.375.263-.63.264-1.319.005-1.949s-.75-1.121-1.377-1.383c-.624-.26-1.29-.249-1.878-.03l1.523.63c.956.4 1.409 1.5 1.009 2.455-.397.957-1.497 1.41-2.454 1.012H7.54zm11.415-9.303c0-1.662-1.353-3.015-3.015-3.015-1.665 0-3.015 1.353-3.015 3.015 0 1.665 1.35 3.015 3.015 3.015 1.663 0 3.015-1.35 3.015-3.015zm-5.273-.005c0-1.252 1.013-2.266 2.265-2.266 1.249 0 2.266 1.014 2.266 2.266 0 1.251-1.017 2.265-2.266 2.265-1.253 0-2.265-1.014-2.265-2.265z",
    riot: "M13.458.86 0 7.093l3.353 12.761 2.552-.313-.701-8.024.838-.373 1.447 8.202 4.361-.535-.775-8.857.83-.37 1.591 9.025 4.412-.542-.849-9.708.84-.374 1.74 9.87L24 17.318V3.5Zm.316 19.356.222 1.256L24 23.14v-4.18l-10.22 1.256Z",
  };

  function startDrag(e: MouseEvent) {
    if ((e.target as HTMLElement).closest("button")) return;
    getCurrentWindow().startDragging();
  }

  function minimize() {
    invoke("minimize_window");
  }

  function toggleMaximize() {
    invoke("toggle_maximize_window");
  }

  function close() {
    invoke("close_window");
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="titlebar" onmousedown={startDrag}>
  <div class="left">
    <button class="btn" onclick={onRefresh} title={translate(locale, "titlebar.refresh")}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" />
        <path d="M21 3v5h-5" />
      </svg>
    </button>

    <button class="btn" onclick={onAddAccount} title={translate(locale, "titlebar.addAccount")}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <line x1="12" y1="5" x2="12" y2="19" />
        <line x1="5" y1="12" x2="19" y2="12" />
      </svg>
    </button>

    <button class="btn" onclick={onOpenSettings} title={translate(locale, "titlebar.settings")}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
        <circle cx="12" cy="12" r="3" />
      </svg>
    </button>
  </div>

  {#if enabledPlatforms.length > 1}
    <div class="tabs">
      {#each enabledPlatforms as platform}
        <button
          class="tab"
          class:active={activeTab === platform.id}
          onclick={() => onTabChange(platform.id)}
          title={platform.name}
          style={activeTab === platform.id ? `color: ${platform.accent};` : ""}
        >
          {#if LOGO_PATHS[platform.id]}
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
              <path d={LOGO_PATHS[platform.id]} />
            </svg>
          {:else}
            <span class="tab-text">{platform.name.slice(0, 2)}</span>
          {/if}
        </button>
      {/each}
    </div>
  {/if}

  <div class="right">
    {#if updateCtaLabel}
      <button
        class="update-btn"
        onclick={onApplyUpdate}
        title={updateCtaTitle}
        disabled={updateCtaDisabled}
      >
        {updateCtaLabel}
      </button>
    {/if}

    <button class="win-btn" onclick={minimize} title={translate(locale, "titlebar.minimize")}>
      <svg width="12" height="12" viewBox="0 0 12 12">
        <rect x="1" y="5.5" width="10" height="1" fill="currentColor" />
      </svg>
    </button>

    <button class="win-btn" onclick={toggleMaximize} title={translate(locale, "titlebar.maximize")}>
      <svg width="12" height="12" viewBox="0 0 12 12">
        <rect x="1.6" y="1.6" width="8.8" height="8.8" fill="none" stroke="currentColor" stroke-width="1.2" />
      </svg>
    </button>

    <button class="win-btn close" onclick={close} title={translate(locale, "titlebar.close")}>
      <svg width="12" height="12" viewBox="0 0 12 12">
        <path d="M1 1l10 10M11 1L1 11" stroke="currentColor" stroke-width="1.2" />
      </svg>
    </button>
  </div>
</div>

<style>
  .titlebar {
    height: 36px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 0 0 10px;
    background: var(--bg);
    user-select: none;
    -webkit-user-select: none;
    border-bottom: 1px solid var(--bg-card);
    position: relative;
  }

  .left {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
  }

  .tabs {
    position: absolute;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .tab {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 28px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: #52525b;
    cursor: pointer;
    transition: all 120ms ease-out;
  }

  .tab:hover {
    background: var(--bg-card);
    color: var(--fg-muted);
  }

  .tab.active {
    background: var(--bg-card);
  }

  .tab-text {
    font-size: 11px;
    font-weight: 600;
  }

  .btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
    transition: all 120ms ease-out;
  }

  .btn:hover {
    background: var(--bg-muted);
    color: var(--fg);
  }

  .btn:active {
    transform: scale(0.92);
  }

  .right {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
    justify-content: flex-end;
    padding-right: 2px;
  }

  .update-btn {
    height: 24px;
    border: 1px solid color-mix(in srgb, var(--fg) 25%, var(--border));
    border-radius: 999px;
    background: color-mix(in srgb, var(--bg-card) 92%, var(--fg) 8%);
    color: var(--fg);
    font-size: 11px;
    font-weight: 600;
    line-height: 1;
    padding: 0 10px;
    cursor: pointer;
    transition: all 120ms ease-out;
  }

  .update-btn:hover {
    background: color-mix(in srgb, var(--bg-card) 70%, var(--fg) 30%);
  }

  .update-btn:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .win-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    border: none;
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
    transition: background 120ms;
  }

  .win-btn:hover {
    background: var(--bg-muted);
    color: var(--fg);
  }

  .win-btn.close:hover {
    background: var(--danger);
    color: var(--fg);
  }
</style>
