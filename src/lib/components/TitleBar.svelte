<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  let {
    onRefresh,
    onAddAccount,
    onOpenSettings,
    onOpenNotifications,
    notifCount = 0,
    activeTab = "steam",
    onTabChange,
    accentColor,
  }: {
    onRefresh: () => void;
    onAddAccount: () => void;
    onOpenSettings: () => void;
    onOpenNotifications: () => void;
    notifCount?: number;
    activeTab: string;
    onTabChange: (tab: string) => void;
    accentColor: string;
  } = $props();

  function startDrag(e: MouseEvent) {
    if ((e.target as HTMLElement).closest("button")) return;
    getCurrentWindow().startDragging();
  }

  function minimize() {
    invoke("minimize_window");
  }

  function close() {
    invoke("close_window");
  }
</script>

<div class="titlebar" onmousedown={startDrag}>
  <!-- Left: app name + actions -->
  <div class="left">
    <span class="app-name">zazaSwitcher</span>

    <button class="btn" onclick={onRefresh} title="Refresh">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" />
        <path d="M21 3v5h-5" />
      </svg>
    </button>

    <button class="btn" onclick={onAddAccount} title="Add account">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <line x1="12" y1="5" x2="12" y2="19" />
        <line x1="5" y1="12" x2="19" y2="12" />
      </svg>
    </button>

    <button class="btn" onclick={onOpenSettings} title="Settings">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
        <circle cx="12" cy="12" r="3" />
      </svg>
    </button>
  </div>

  <!-- Center: platform tabs -->
  <div class="tabs">
    <button
      class="tab"
      class:active={activeTab === "steam"}
      onclick={() => onTabChange("steam")}
      title="Steam"
      style={activeTab === "steam" ? `color: ${accentColor};` : ""}
    >
      <!-- Steam logo -->
      <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
        <path d="M12 2a10 10 0 0 0-9.96 9.04l5.35 2.21a2.83 2.83 0 0 1 1.6-.5l.01 0 2.39-3.46v-.05a3.78 3.78 0 1 1 3.78 3.78h-.09l-3.4 2.43a2.85 2.85 0 0 1-2.84 2.74 2.85 2.85 0 0 1-2.82-2.42L2.26 14.5A10 10 0 1 0 12 2zm-4.96 14.88a2.14 2.14 0 0 0 1.22 2.73 2.14 2.14 0 0 0 2.74-1.22 2.14 2.14 0 0 0-1.22-2.74l-1.17-.48c.36-.27.8-.43 1.28-.43a2.14 2.14 0 1 1-2.14 2.14l-.71.0zm8.72-7.63a2.52 2.52 0 1 0-2.52-2.52 2.52 2.52 0 0 0 2.52 2.52z"/>
      </svg>
    </button>

    <button
      class="tab"
      class:active={activeTab === "riot"}
      onclick={() => onTabChange("riot")}
      title="Riot Games"
      style={activeTab === "riot" ? `color: ${accentColor};` : ""}
    >
      <!-- Riot fist logo (simplified) -->
      <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
        <path d="M3.06 7.04L7.5 4.5l6 1.5 4.5-1.5 3 2.25-1.5 10.5-3 1.5H9l-2.25 1.5H4.5l-.75-3L3.06 7.04zM9 15h7.5l1.13-7.5L13.5 6l-6-1.5-2.81 1.6L5.25 15l.75 1.5h.75L9 15z"/>
      </svg>
    </button>
  </div>

  <!-- Right: notifications + window controls -->
  <div class="right">
    <button class="btn notif-btn" onclick={onOpenNotifications} title="Notifications">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" />
        <path d="M10.3 21a1.94 1.94 0 0 0 3.4 0" />
      </svg>
      {#if notifCount > 0}
        <span class="notif-dot" style="background: {accentColor};"></span>
      {/if}
    </button>

    <button class="win-btn" onclick={minimize} title="Minimize">
      <svg width="12" height="12" viewBox="0 0 12 12">
        <rect x="1" y="5.5" width="10" height="1" fill="currentColor" />
      </svg>
    </button>

    <button class="win-btn close" onclick={close} title="Close">
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
    padding: 0 4px 0 12px;
    background: #09090b;
    user-select: none;
    -webkit-user-select: none;
    border-bottom: 1px solid #1c1c1f;
  }

  .left {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
  }

  .app-name {
    font-size: 12px;
    font-weight: 500;
    color: #a1a1aa;
    margin-right: 4px;
  }

  .tabs {
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
    background: #1c1c1f;
    color: #a1a1aa;
  }

  .tab.active {
    background: #1c1c1f;
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
    color: #a1a1aa;
    cursor: pointer;
    transition: all 120ms ease-out;
  }

  .btn:hover {
    background: #27272a;
    color: #fafafa;
  }

  .btn:active {
    transform: scale(0.92);
  }

  .notif-btn {
    position: relative;
  }

  .notif-dot {
    position: absolute;
    top: 4px;
    right: 4px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
  }

  .right {
    display: flex;
    align-items: center;
    gap: 2px;
    flex: 1;
    justify-content: flex-end;
  }

  .win-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    border: none;
    background: transparent;
    color: #a1a1aa;
    cursor: pointer;
    transition: background 120ms;
  }

  .win-btn:hover {
    background: #27272a;
    color: #fafafa;
  }

  .win-btn.close:hover {
    background: #dc2626;
    color: #fafafa;
  }
</style>
