<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type { PlatformDef } from "../../features/settings/types";
  import { DEFAULT_LOCALE, translate, type Locale } from "$lib/i18n";

  let {
    onRefresh,
    onAddAccount,
    onOpenSettings,
    onBulkEdit = () => {},
    onApplyUpdate = () => {},
    activeTab = "steam",
    onTabChange,
    enabledPlatforms,
    unavailablePlatformIds = new Set<string>(),
    canRefresh = true,
    canAddAccount = true,
    showBulkEdit = false,
    bulkEditActive = false,
    updateCtaLabel = null,
    updateCtaTitle = "Restart to apply update",
    updateCtaDisabled = false,
    locale = DEFAULT_LOCALE,
  }: {
    onRefresh: () => void;
    onAddAccount: () => void;
    onOpenSettings: () => void;
    onBulkEdit?: () => void;
    onApplyUpdate?: () => void;
    activeTab: string;
    onTabChange: (tab: string) => void;
    enabledPlatforms: PlatformDef[];
    unavailablePlatformIds?: Set<string>;
    canRefresh?: boolean;
    canAddAccount?: boolean;
    showBulkEdit?: boolean;
    bulkEditActive?: boolean;
    updateCtaLabel?: string | null;
    updateCtaTitle?: string;
    updateCtaDisabled?: boolean;
    locale?: Locale;
  } = $props();
  const TAB_ICON_PATHS: Record<string, string> = {
    steam: "M11.979 0C5.678 0 .511 4.86.022 11.037l6.432 2.658c.545-.371 1.203-.59 1.912-.59.063 0 .125.004.188.006l2.861-4.142V8.91c0-2.495 2.028-4.524 4.524-4.524 2.494 0 4.524 2.031 4.524 4.527s-2.03 4.525-4.524 4.525h-.105l-4.076 2.911c0 .052.004.105.004.159 0 1.875-1.515 3.396-3.39 3.396-1.635 0-3.016-1.173-3.331-2.727L.436 15.27C1.862 20.307 6.486 24 11.979 24c6.627 0 11.999-5.373 11.999-12S18.605 0 11.979 0zM7.54 18.21l-1.473-.61c.262.543.714.999 1.314 1.25 1.297.539 2.793-.076 3.332-1.375.263-.63.264-1.319.005-1.949s-.75-1.121-1.377-1.383c-.624-.26-1.29-.249-1.878-.03l1.523.63c.956.4 1.409 1.5 1.009 2.455-.397.957-1.497 1.41-2.454 1.012H7.54zm11.415-9.303c0-1.662-1.353-3.015-3.015-3.015-1.665 0-3.015 1.353-3.015 3.015 0 1.665 1.35 3.015 3.015 3.015 1.663 0 3.015-1.35 3.015-3.015zm-5.273-.005c0-1.252 1.013-2.266 2.265-2.266 1.249 0 2.266 1.014 2.266 2.266 0 1.251-1.017 2.265-2.266 2.265-1.253 0-2.265-1.014-2.265-2.265z",
    riot: "M13.458.86 0 7.093l3.353 12.761 2.552-.313-.701-8.024.838-.373 1.447 8.202 4.361-.535-.775-8.857.83-.37 1.591 9.025 4.412-.542-.849-9.708.84-.374 1.74 9.87L24 17.318V3.5Zm.316 19.356.222 1.256L24 23.14v-4.18l-10.22 1.256Z",
    "battle-net": "M18.94 8.296C15.9 6.892 11.534 6 7.426 6.332c.206-1.36.714-2.308 1.548-2.508 1.148-.275 2.4.48 3.594 1.854.782.102 1.71.28 2.355.429C12.747 2.013 9.828-.282 7.607.565c-1.688.644-2.553 2.97-2.448 6.094-2.2.468-3.915 1.3-5.013 2.495-.056.065-.181.227-.137.305.034.058.146-.008.194-.04 1.274-.89 2.904-1.373 5.027-1.676.303 3.333 1.713 7.56 4.055 10.952-1.28.502-2.356.536-2.946-.087-.812-.856-.784-2.318-.19-4.04a26.764 26.764 0 0 1-.807-2.254c-2.459 3.934-2.986 7.61-1.143 9.11 1.402 1.14 3.847.725 6.502-.926 1.505 1.672 3.083 2.74 4.667 3.094.084.015.287.043.332-.034.034-.06-.08-.124-.131-.149-1.408-.657-2.64-1.828-3.964-3.515 2.735-1.929 5.691-5.263 7.457-8.988 1.076.86 1.64 1.773 1.398 2.595-.336 1.131-1.615 1.84-3.403 2.185a27.697 27.697 0 0 1-1.548 1.826c4.634.16 8.08-1.22 8.458-3.565.286-1.786-1.295-3.696-4.053-5.17.696-2.139.832-4.04.346-5.588-.029-.08-.106-.27-.196-.27-.068 0-.067.13-.063.187.135 1.547-.263 3.2-1.062 5.19zm-8.533 9.869c-1.96-3.145-3.09-6.849-3.082-10.594 3.702-.124 7.474.748 10.714 2.627-1.743 3.269-4.385 6.1-7.633 7.966h.001z",
    ubisoft: "M23.561 11.988C23.301-.304 6.954-4.89.656 6.634c.282.206.661.477.943.672a11.747 11.747 0 00-.976 3.067 11.885 11.885 0 00-.184 2.071C.439 18.818 5.621 24 12.005 24c6.385 0 11.556-5.17 11.556-11.556v-.455zm-20.27 2.06c-.152 1.246-.054 1.636-.054 1.788l-.282.098c-.108-.206-.37-.932-.488-1.908C2.163 10.308 4.7 6.96 8.57 6.33c3.544-.52 6.937 1.68 7.728 4.758l-.282.098c-.087-.087-.228-.336-.77-.878-4.281-4.281-11.002-2.32-11.956 3.74zm11.002 2.081a3.145 3.145 0 01-2.59 1.355 3.15 3.15 0 01-3.155-3.155 3.159 3.159 0 012.927-3.144c1.018-.043 1.972.51 2.416 1.398a2.58 2.58 0 01-.455 2.95c.293.205.575.4.856.595zm6.58.12c-1.669 3.782-5.106 5.766-8.77 5.712-7.034-.347-9.083-8.466-4.38-11.393l.207.206c-.076.108-.358.325-.791 1.182-.51 1.041-.672 2.081-.607 2.732.369 5.67 8.314 6.83 11.045 1.214C21.057 8.217 11.822.401 3.626 6.374l-.184-.184C5.599 2.808 9.816 1.3 13.837 2.309c6.147 1.55 9.453 7.956 7.035 13.94z",
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
    <button class="btn" onclick={onRefresh} title={translate(locale, "titlebar.refresh")} disabled={!canRefresh}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" />
        <path d="M21 3v5h-5" />
      </svg>
    </button>

    <button class="btn" onclick={onAddAccount} title={translate(locale, "titlebar.addAccount")} disabled={!canAddAccount}>
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

    {#if showBulkEdit}
      <button class="btn" class:active-mode={bulkEditActive} onclick={onBulkEdit} title={translate(locale, "bulkEdit.title")}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
          <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
        </svg>
      </button>
    {/if}
  </div>

  {#if enabledPlatforms.length > 1}
    <div class="tabs">
      {#each enabledPlatforms as platform}
        {@const unavailable = unavailablePlatformIds.has(platform.id)}
        {@const tabIconPath = TAB_ICON_PATHS[platform.id]}
        <button
          class="tab"
          class:active={activeTab === platform.id}
          class:disabled={unavailable}
          onclick={() => onTabChange(platform.id)}
          title={unavailable ? `${platform.name} (${translate(locale, "settings.platformUnsupportedOs")})` : platform.name}
          style={activeTab === platform.id ? `color: ${platform.accent};` : ""}
          disabled={unavailable}
        >
          {#if tabIconPath}
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
              <path d={tabIconPath} />
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

  .tab.disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .tab.disabled:hover {
    background: transparent;
    color: #52525b;
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

  .btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .btn:disabled:hover {
    background: transparent;
    color: var(--fg-muted);
  }

  .btn.active-mode {
    background: color-mix(in srgb, #2563eb 24%, var(--bg-muted));
    color: #60a5fa;
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
