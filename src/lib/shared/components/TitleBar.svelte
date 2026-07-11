<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type { PlatformDef } from "../platform";
  import { PLATFORM_ICON_PATHS, PERSONAS_ICON_PATH, PERSONAS_ACCENT } from "../platformIcons";
  import { DEFAULT_LOCALE, translate, type Locale } from "$lib/i18n";

  let {
    onRefresh,
    onAddAccount,
    onOpenSettings,
    onOpenPersonas = () => {},
    personasActive = false,
    personasVisible = true,
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
    showSettings = false,
    updateCtaDisabled = false,
    locale = DEFAULT_LOCALE,
    runtimeOs = "unknown",
  }: {
    onRefresh: () => void;
    onAddAccount: () => void;
    onOpenSettings: () => void;
    onOpenPersonas?: () => void;
    personasActive?: boolean;
    personasVisible?: boolean;
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
    showSettings?: boolean;
    updateCtaLabel?: string | null;
    updateCtaTitle?: string;
    updateCtaDisabled?: boolean;
    locale?: Locale;
    runtimeOs?: "windows" | "linux" | "macos" | "unknown";
  } = $props();

  const isMacOs = $derived(runtimeOs === "macos");
  let isMaximized = $state(false);

  onMount(() => {
    const win = getCurrentWindow();
    win.isMaximized().then((v) => (isMaximized = v));
    const unlisten = win.onResized(async () => {
      isMaximized = await win.isMaximized();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  });

  function startDrag(e: MouseEvent) {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button")) return;
    if (e.detail === 2) {
      toggleMaximize();
      return;
    }
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

{#snippet actionButtons()}
  <button class="btn" onclick={onRefresh} title={translate(locale, "titlebar.refresh")} aria-label={translate(locale, "titlebar.refresh")} disabled={!canRefresh}>
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" />
      <path d="M21 3v5h-5" />
    </svg>
  </button>

  <button class="btn" data-tour="add-account" onclick={onAddAccount} title={translate(locale, "titlebar.addAccount")} aria-label={translate(locale, "titlebar.addAccount")} disabled={!canAddAccount}>
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="5" y1="12" x2="19" y2="12" />
    </svg>
  </button>

  <button class="btn" onclick={onOpenSettings} title={translate(locale, "titlebar.settings")} aria-label={translate(locale, "titlebar.settings")}>
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  </button>

  {#if showBulkEdit}
    <button class="btn" class:active-mode={bulkEditActive} onclick={onBulkEdit} title={translate(locale, "bulkEdit.title")} aria-label={translate(locale, "bulkEdit.title")} aria-pressed={bulkEditActive}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
        <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
      </svg>
    </button>
  {/if}
{/snippet}

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="titlebar" class:macos={isMacOs} onmousedown={startDrag}>
  <div class="left">
    {#if !isMacOs}
      {@render actionButtons()}
    {/if}
  </div>

  {#if enabledPlatforms.length > 1 || personasVisible}
    <div class="tabs" data-tour="platforms" role="tablist">
      {#if personasVisible}
        <!-- Personas behaves like a platform: first tab, same look, own accent. -->
        <div class="tab-wrap">
          <button
            class="tab"
            class:active={personasActive}
            onclick={onOpenPersonas}
            style={personasActive ? `color: ${PERSONAS_ACCENT};` : ""}
            role="tab"
            aria-label={translate(locale, "titlebar.personas")}
            aria-selected={personasActive}
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
              <path d={PERSONAS_ICON_PATH} />
            </svg>
          </button>
          <span class="tab-tooltip">{translate(locale, "titlebar.personas")}</span>
        </div>
      {/if}
      {#each enabledPlatforms as platform}
        {@const unavailable = unavailablePlatformIds.has(platform.id)}
        {@const tabIconPath = PLATFORM_ICON_PATHS[platform.id]}
        {@const tabActive = !showSettings && !personasActive && activeTab === platform.id}
        <div class="tab-wrap">
          <button
            class="tab"
            class:active={tabActive}
            class:disabled={unavailable}
            onclick={() => onTabChange(platform.id)}
            style={tabActive ? `color: ${platform.accent};` : ""}
            disabled={unavailable}
            role="tab"
            aria-label={platform.name}
            aria-selected={tabActive}
          >
            {#if tabIconPath}
              <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                <path d={tabIconPath} />
              </svg>
            {:else}
              <span class="tab-text">{platform.name.slice(0, 2)}</span>
            {/if}
          </button>
          <span class="tab-tooltip">{unavailable ? `${platform.name} (${translate(locale, "settings.platformUnsupportedOs")})` : platform.name}</span>
        </div>
      {/each}
    </div>
  {/if}

  <div class="right">
    {#if isMacOs}
      {@render actionButtons()}
    {/if}

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

    {#if !isMacOs}
      <button class="win-btn" onclick={minimize} title={translate(locale, "titlebar.minimize")}>
        <svg width="12" height="12" viewBox="0 0 12 12">
          <rect x="1" y="5.5" width="10" height="1" fill="currentColor" />
        </svg>
      </button>

      <button class="win-btn" onclick={toggleMaximize} title={translate(locale, isMaximized ? "titlebar.restore" : "titlebar.maximize")}>
        {#if isMaximized}
          <svg width="12" height="12" viewBox="0 0 12 12">
            <rect x="1.6" y="3.4" width="7" height="7" fill="none" stroke="currentColor" stroke-width="1.2" />
            <path d="M3.4 3.4V1.6h7v7H8.6" fill="none" stroke="currentColor" stroke-width="1.2" />
          </svg>
        {:else}
          <svg width="12" height="12" viewBox="0 0 12 12">
            <rect x="1.6" y="1.6" width="8.8" height="8.8" fill="none" stroke="currentColor" stroke-width="1.2" />
          </svg>
        {/if}
      </button>

      <button class="win-btn close" onclick={close} title={translate(locale, "titlebar.close")}>
        <svg width="12" height="12" viewBox="0 0 12 12">
          <path d="M1 1l10 10M11 1L1 11" stroke="currentColor" stroke-width="1.2" />
        </svg>
      </button>
    {/if}
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
    /* No own background: .app-shell paints var(--bg) behind. A second var(--bg)
       layer here would double the alpha on translucent themes. */
    background: transparent;
    user-select: none;
    -webkit-user-select: none;
    border-bottom: 1px solid var(--bg-card);
    position: relative;
  }

  /* macOS: native traffic lights float at top-left inside our titlebar.
     72px of left padding clears them. 32px matches native title-bar height.
     Opaque background (ignores --bg-opacity) so the desktop never bleeds
     through the titlebar when the window is transparent. */
  .titlebar.macos {
    height: 32px;
    padding-left: 72px;
    background: var(--bg-solid);
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
    /* With every platform enabled in a narrow window the centered tabs would
       run under the action buttons and steal their clicks. Cap and scroll. */
    max-width: min(46vw, calc(100% - 230px));
    overflow-x: auto;
    scrollbar-width: none;
  }

  .tabs::-webkit-scrollbar {
    display: none;
  }

  .tab-wrap {
    position: relative;
  }

  .tab-tooltip {
    position: absolute;
    left: 50%;
    top: 100%;
    transform: translateX(-50%) translateY(4px);
    padding: 4px 8px;
    background: var(--bg-overlay);
    border: 1px solid var(--border);
    border-radius: 4px;
    font-size: 11px;
    font-weight: 500;
    color: var(--fg);
    white-space: nowrap;
    pointer-events: none;
    opacity: 0;
    transition: opacity 120ms ease-out;
    z-index: 100;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  }

  .tab-wrap:hover .tab-tooltip,
  .tab-wrap:focus-within .tab-tooltip {
    opacity: 1;
    transition-delay: 300ms;
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
    color: var(--fg-subtle);
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
    color: var(--fg-subtle);
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
