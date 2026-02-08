<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import AccountCard from "$lib/components/AccountCard.svelte";
  import TitleBar from "$lib/components/TitleBar.svelte";
  import ContextMenu from "$lib/components/ContextMenu.svelte";
  import Settings from "$lib/components/Settings.svelte";
  import Toast from "$lib/components/Toast.svelte";
  import NotificationPanel from "$lib/components/NotificationPanel.svelte";
  import { toProfileUrl } from "$lib/steamIdUtils";
  import { encodeFriendCode } from "$lib/friendCode";
  import { getUnreadCount, addNotification } from "$lib/notifications";
  import { getCachedAvatar } from "$lib/avatarCache";

  interface SteamAccount {
    steam_id: string;
    account_name: string;
    persona_name: string;
  }

  // Accent colors per tab
  const ACCENT: Record<string, string> = {
    steam: "#3b82f6",
    riot: "#ef4444",
  };

  let activeTab = $state("steam");
  let accentColor = $derived(ACCENT[activeTab]);

  let accounts = $state<SteamAccount[]>([]);
  let currentAccount = $state("");
  let loading = $state(true);
  let switching = $state(false);
  let error = $state<string | null>(null);

  let refreshAvatarFor = $state<string | null>(null);

  // Context menu
  let contextMenu = $state<{ x: number; y: number; account: SteamAccount } | null>(null);

  // Panels
  let showSettings = $state(false);
  let showNotifications = $state(false);

  // Toast
  let toastMessage = $state<string | null>(null);

  // Notification count
  let notifCount = $state(getUnreadCount());

  // Smooth centering
  let wrapperRef = $state<HTMLDivElement | null>(null);
  let paddingLeft = $state(0);
  let isResizing = $state(false);
  let resizeTimeout: number;

  const CARD_WIDTH = 120;
  const GAP = 12;

  function calculatePadding() {
    if (!wrapperRef) return;
    const availableWidth = wrapperRef.clientWidth;
    const cardsPerRow = Math.floor((availableWidth + GAP) / (CARD_WIDTH + GAP));
    if (cardsPerRow < 1) return;
    const totalCardsWidth = cardsPerRow * CARD_WIDTH + (cardsPerRow - 1) * GAP;
    paddingLeft = Math.floor((availableWidth - totalCardsWidth) / 2);
  }

  function handleResize() {
    isResizing = true;
    clearTimeout(resizeTimeout);
    resizeTimeout = setTimeout(() => {
      isResizing = false;
      calculatePadding();
    }, 200);
  }

  function showToast(msg: string) {
    toastMessage = msg;
  }

  async function copyToClipboard(text: string, label: string) {
    await navigator.clipboard.writeText(text);
    showToast(`${label} copied`);
  }

  function checkAvatarChanges(oldAccounts: SteamAccount[]) {
    for (const account of oldAccounts) {
      const cached = getCachedAvatar(account.steam_id);
      if (cached && cached.expired) {
        addNotification(`Profile picture updated for ${account.persona_name || account.account_name}`);
        notifCount = getUnreadCount();
      }
    }
  }

  async function loadAccounts() {
    loading = true;
    error = null;

    try {
      const oldAccounts = [...accounts];
      accounts = await invoke<SteamAccount[]>("get_steam_accounts");
      currentAccount = await invoke<string>("get_current_account");

      if (oldAccounts.length > 0) {
        checkAvatarChanges(oldAccounts);
      }
    } catch (e) {
      error = String(e);
    }

    loading = false;
    setTimeout(calculatePadding, 0);
  }

  async function switchAccount(username: string, steamId: string) {
    if (switching || username === currentAccount) return;

    switching = true;
    error = null;

    try {
      await invoke("switch_account", { username });
      currentAccount = username;

      refreshAvatarFor = steamId;
      setTimeout(() => { refreshAvatarFor = null; }, 100);
    } catch (e) {
      error = String(e);
    }

    switching = false;
  }

  async function switchAccountMode(username: string, steamId: string, mode: string) {
    if (switching) return;

    switching = true;
    error = null;

    try {
      await invoke("switch_account_mode", { username, steamId, mode });
      currentAccount = username;

      refreshAvatarFor = steamId;
      setTimeout(() => { refreshAvatarFor = null; }, 100);
    } catch (e) {
      error = String(e);
    }

    switching = false;
  }

  async function addAccount() {
    try {
      await invoke("add_account");
    } catch (e) {
      error = String(e);
    }
  }

  function openContextMenu(e: MouseEvent, account: SteamAccount) {
    contextMenu = { x: e.clientX, y: e.clientY, account };
  }

  function getContextMenuItems(account: SteamAccount) {
    return [
      {
        label: "Launch online",
        action: () => switchAccountMode(account.account_name, account.steam_id, "online"),
      },
      {
        label: "Launch invisible",
        action: () => switchAccountMode(account.account_name, account.steam_id, "invisible"),
      },
      { separator: true as const },
      {
        label: "Copy SteamID64",
        action: () => copyToClipboard(account.steam_id, "SteamID64"),
      },
      {
        label: "Copy Friend Code",
        action: () => {
          const code = encodeFriendCode(account.steam_id);
          copyToClipboard(code, "Friend Code");
        },
      },
      {
        label: "Copy profile URL",
        action: () => copyToClipboard(toProfileUrl(account.steam_id), "Profile URL"),
      },
      { separator: true as const },
      {
        label: "Open userdata folder",
        action: async () => {
          try {
            await invoke("open_userdata", { steamId: account.steam_id });
          } catch (e) {
            showToast(String(e));
          }
        },
      },
    ];
  }

  onMount(() => {
    loadAccounts();
    window.addEventListener("resize", handleResize);
  });

  onDestroy(() => {
    window.removeEventListener("resize", handleResize);
    clearTimeout(resizeTimeout);
  });
</script>

<div class="app-shell" style="border-color: {accentColor}20;">
<TitleBar
  onRefresh={loadAccounts}
  onAddAccount={addAccount}
  onOpenSettings={() => showSettings = true}
  onOpenNotifications={() => { showNotifications = true; notifCount = 0; }}
  {notifCount}
  {activeTab}
  onTabChange={(tab) => activeTab = tab}
  {accentColor}
/>

{#if activeTab === "steam"}
  <main class="content" style="background: #09090b; color: #fafafa;">

    {#if error}
      <div class="mb-4 p-3 rounded-lg text-sm" style="background: rgba(239,68,68,0.1); color: #f87171;">
        {error}
      </div>
    {/if}

    {#if loading}
      <div class="flex flex-col items-center justify-center py-12" style="color: #a1a1aa;">
        <div class="spinner mb-3" style="border-top-color: {accentColor};" />
        <p class="text-sm">Loading...</p>
      </div>

    {:else if accounts.length === 0}
      <div class="text-center py-12" style="color: #a1a1aa;">
        <p>No Steam accounts found</p>
        <p class="text-sm mt-1 opacity-70">Make sure Steam is installed and you have logged in at least once.</p>
      </div>

    {:else}
      <div bind:this={wrapperRef} class="w-full">
        <div
          class="grid-container"
          style="padding-left: {paddingLeft}px; {isResizing ? '' : 'transition: padding-left 200ms ease-out;'}"
        >
          {#each accounts as account (account.steam_id)}
            <AccountCard
              {account}
              isActive={account.account_name === currentAccount}
              onSwitch={() => switchAccount(account.account_name, account.steam_id)}
              onContextMenu={(e) => openContextMenu(e, account)}
              refreshAvatar={refreshAvatarFor === account.steam_id}
            />
          {/each}
        </div>
      </div>
    {/if}

    {#if switching}
      <div class="fixed inset-0 flex items-center justify-center z-50" style="background: rgba(9,9,11,0.9); backdrop-filter: blur(4px);">
        <div class="p-6 text-center rounded-lg" style="background: #1c1c1f;">
          <div class="spinner mx-auto mb-3" style="border-top-color: {accentColor};" />
          <p class="text-sm font-medium">Switching account...</p>
          <p class="text-xs mt-1" style="color: #a1a1aa;">Steam is restarting</p>
        </div>
      </div>
    {/if}

  </main>

{:else if activeTab === "riot"}
  <main class="content" style="background: #09090b; color: #fafafa;">
    <div class="flex flex-col items-center justify-center py-12" style="color: #a1a1aa;">
      <p class="text-sm">Riot Games - Coming soon</p>
    </div>
  </main>
{/if}
</div>

{#if contextMenu}
  <ContextMenu
    items={getContextMenuItems(contextMenu.account)}
    x={contextMenu.x}
    y={contextMenu.y}
    onClose={() => contextMenu = null}
  />
{/if}

{#if showSettings}
  <Settings onClose={() => showSettings = false} />
{/if}

{#if showNotifications}
  <NotificationPanel onClose={() => showNotifications = false} />
{/if}

{#if toastMessage}
  <Toast message={toastMessage} onDone={() => toastMessage = null} />
{/if}

<style>
  .app-shell {
    height: 100vh;
    display: flex;
    flex-direction: column;
    border: 1px solid #27272a;
    box-sizing: border-box;
    transition: border-color 300ms ease-out;
  }

  .content {
    flex: 1;
    padding: 16px;
    overflow-y: auto;
  }

  .grid-container {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
  }

  .spinner {
    width: 20px;
    height: 20px;
    border: 2px solid #27272a;
    border-top-color: #3b82f6;
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
