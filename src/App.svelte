<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import AccountCard from "$lib/components/AccountCard.svelte";

  interface SteamAccount {
    steam_id: string;
    account_name: string;
    persona_name: string;
  }

  let accounts = $state<SteamAccount[]>([]);
  let currentAccount = $state("");
  let loading = $state(true);
  let switching = $state(false);
  let error = $state<string | null>(null);

  // Track which account needs avatar refresh after switch
  let refreshAvatarFor = $state<string | null>(null);

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

  async function loadAccounts() {
    loading = true;
    error = null;

    try {
      accounts = await invoke<SteamAccount[]>("get_steam_accounts");
      currentAccount = await invoke<string>("get_current_account");
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

      // Trigger avatar refresh for this account
      refreshAvatarFor = steamId;
      // Reset after a tick so the effect triggers
      setTimeout(() => { refreshAvatarFor = null; }, 100);
    } catch (e) {
      error = String(e);
    }

    switching = false;
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

<main class="min-h-screen p-4" style="background: #09090b; color: #fafafa;">

  {#if error}
    <div class="mb-4 p-3 rounded-lg text-sm" style="background: rgba(239,68,68,0.1); color: #f87171;">
      {error}
    </div>
  {/if}

  {#if loading}
    <div class="flex flex-col items-center justify-center py-12" style="color: #a1a1aa;">
      <div class="spinner mb-3" />
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
            refreshAvatar={refreshAvatarFor === account.steam_id}
          />
        {/each}
      </div>
    </div>
  {/if}

  {#if switching}
    <div class="fixed inset-0 flex items-center justify-center z-50" style="background: rgba(9,9,11,0.9); backdrop-filter: blur(4px);">
      <div class="p-6 text-center rounded-lg" style="background: #1c1c1f;">
        <div class="spinner mx-auto mb-3" />
        <p class="text-sm font-medium">Switching account...</p>
        <p class="text-xs mt-1" style="color: #a1a1aa;">Steam is restarting</p>
      </div>
    </div>
  {/if}

</main>

<style>
  .grid-container {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
  }

  .spinner {
    width: 20px;
    height: 20px;
    border: 2px solid #27272a;
    border-top-color: #fafafa;
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
