<script lang="ts">
  import { onMount } from "svelte";
  import { getCachedAvatar, fetchAvatar } from "$lib/avatarCache";

  let { account, isActive, onSwitch, refreshAvatar = false, onContextMenu }: {
    account: { steam_id: string; account_name: string; persona_name: string };
    isActive: boolean;
    onSwitch: () => void;
    refreshAvatar?: boolean;
    onContextMenu: (e: MouseEvent) => void;
  } = $props();

  let avatarUrl = $state<string | null>(null);
  let isLoading = $state(false);
  let isRefreshing = $state(false);
  let showConfirm = $state(false);
  let cardRef = $state<HTMLButtonElement | null>(null);

  async function loadAvatar(forceRefresh = false) {
    const cached = getCachedAvatar(account.steam_id);

    if (cached && !forceRefresh) {
      avatarUrl = cached.url;
      if (cached.expired) {
        isRefreshing = true;
        const newUrl = await fetchAvatar(account.steam_id);
        if (newUrl) avatarUrl = newUrl;
        isRefreshing = false;
      }
    } else if (cached && forceRefresh) {
      avatarUrl = cached.url;
      isRefreshing = true;
      const newUrl = await fetchAvatar(account.steam_id);
      if (newUrl) avatarUrl = newUrl;
      isRefreshing = false;
    } else {
      isLoading = true;
      const url = await fetchAvatar(account.steam_id);
      avatarUrl = url;
      isLoading = false;
    }
  }

  onMount(() => {
    loadAvatar();

    // Dismiss play overlay on any click outside this card
    function onDocClick(e: MouseEvent) {
      if (showConfirm && cardRef && !cardRef.contains(e.target as Node)) {
        showConfirm = false;
      }
    }
    document.addEventListener("mousedown", onDocClick);
    return () => document.removeEventListener("mousedown", onDocClick);
  });

  $effect(() => {
    if (refreshAvatar) {
      loadAvatar(true);
    }
  });

  function getInitials(name: string): string {
    return name.slice(0, 2).toUpperCase();
  }

  function handleClick() {
    if (isActive) return;

    if (showConfirm) {
      showConfirm = false;
      onSwitch();
    } else {
      showConfirm = true;
    }
  }

  function handleContextMenu(e: MouseEvent) {
    e.preventDefault();
    showConfirm = false;
    onContextMenu(e);
  }
</script>

<button
  bind:this={cardRef}
  onclick={handleClick}
  oncontextmenu={handleContextMenu}
  class="card"
  class:active={isActive}
>
  <div class="avatar" class:active={isActive}>
    {#if isLoading}
      <div class="loader"></div>
    {:else if avatarUrl}
      <img
        src={avatarUrl}
        alt={account.persona_name}
        class:blurred={isRefreshing || showConfirm}
      />
      {#if isRefreshing}
        <div class="loader overlay"></div>
      {/if}
    {:else}
      <span class="initials" class:blurred-text={showConfirm}>
        {getInitials(account.persona_name || account.account_name)}
      </span>
    {/if}

    {#if showConfirm}
      <div class="play-overlay">
        <svg width="28" height="28" viewBox="0 0 24 24" fill="#fafafa">
          <path d="M8 5v14l11-7z" />
        </svg>
      </div>
    {/if}
  </div>

  <div class="name">
    {account.persona_name || account.account_name}
  </div>

  <div class="username">
    {account.account_name}
  </div>
</button>

<style>
  .card {
    width: 120px;
    padding: 12px;
    border-radius: 8px;
    text-align: center;
    background: #1c1c1f;
    border: none;
    cursor: pointer;
    transition: all 150ms ease-out;
    color: inherit;
  }

  .card:not(.active):hover {
    background: #252528;
    transform: scale(1.02);
  }

  .card:not(.active):active {
    transform: scale(0.98);
  }

  .card.active {
    background: #252528;
    outline: 2px solid rgba(255, 255, 255, 0.4);
    cursor: default;
  }

  .avatar {
    position: relative;
    width: 80px;
    height: 80px;
    margin: 0 auto 10px;
    border-radius: 6px;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
    background: #27272a;
    transition: background 150ms;
  }

  .avatar img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    transition: filter 300ms ease-out;
  }

  .avatar img.blurred {
    filter: blur(6px) brightness(0.5);
  }

  .avatar .initials {
    font-size: 24px;
    font-weight: 600;
    color: #fafafa;
    transition: filter 300ms ease-out;
  }

  .avatar .initials.blurred-text {
    filter: blur(4px) brightness(0.5);
  }

  .play-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    animation: fadeIn 150ms ease-out;
  }

  @keyframes fadeIn {
    from { opacity: 0; transform: scale(0.8); }
    to { opacity: 1; transform: scale(1); }
  }

  .loader {
    width: 20px;
    height: 20px;
    border: 2px solid #3f3f46;
    border-top-color: #fafafa;
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  .loader.overlay {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
  }

  .card:not(.active):hover .avatar {
    background: #3f3f46;
  }

  .avatar.active {
    outline: 2px solid rgba(255, 255, 255, 0.2);
  }

  .name {
    font-size: 12px;
    font-weight: 500;
    color: #fafafa;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .username {
    font-size: 10px;
    color: #a1a1aa;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
