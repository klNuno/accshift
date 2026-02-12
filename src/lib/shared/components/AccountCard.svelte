<script lang="ts">
  import { onMount } from "svelte";
  import type { PlatformAccount } from "../platform";

  let {
    account,
    isActive,
    onSwitch,
    onContextMenu,
    isDragged = false,
    avatarUrl = null,
    isLoadingAvatar = false,
    isRefreshingAvatar = false,
  }: {
    account: PlatformAccount;
    isActive: boolean;
    onSwitch: () => void;
    onContextMenu: (e: MouseEvent) => void;
    isDragged?: boolean;
    avatarUrl?: string | null;
    isLoadingAvatar?: boolean;
    isRefreshingAvatar?: boolean;
  } = $props();

  let showConfirm = $state(false);
  let cardRef = $state<HTMLDivElement | null>(null);

  onMount(() => {
    function onDocClick(e: MouseEvent) {
      if (showConfirm && cardRef && !cardRef.contains(e.target as Node)) {
        showConfirm = false;
      }
    }
    document.addEventListener("mousedown", onDocClick);
    return () => document.removeEventListener("mousedown", onDocClick);
  });

  $effect(() => {
    if (isDragged) showConfirm = false;
  });

  function getInitials(name: string): string {
    return name.slice(0, 2).toUpperCase();
  }

  function handleClick() {
    if (isActive || isDragged) return;
    if (showConfirm) {
      showConfirm = false;
      onSwitch();
    } else {
      showConfirm = true;
    }
  }

  function handleContextMenu(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    showConfirm = false;
    onContextMenu(e);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={cardRef}
  onclick={handleClick}
  oncontextmenu={handleContextMenu}
  data-account-id={account.id}
  class="card"
  class:active={isActive}
  class:dragging={isDragged}
>
  <div class="avatar" class:active={isActive}>
    {#if isLoadingAvatar}
      <div class="loader"></div>
    {:else if avatarUrl}
      <img
        src={avatarUrl}
        alt={account.displayName}
        class:blurred={isRefreshingAvatar || showConfirm}
      />
      {#if isRefreshingAvatar}
        <div class="loader overlay"></div>
      {/if}
    {:else}
      <span class="initials" class:blurred-text={showConfirm}>
        {getInitials(account.displayName || account.username)}
      </span>
    {/if}

    {#if showConfirm && !isDragged}
      <div class="play-overlay">
        <svg width="28" height="28" viewBox="0 0 24 24" fill="var(--fg)">
          <path d="M8 5v14l11-7z" />
        </svg>
      </div>
    {/if}
  </div>

  <div class="name">
    {account.displayName || account.username}
  </div>

  <div class="username">
    {account.username}
  </div>
</div>

<style>
  .card {
    width: 120px;
    padding: 12px;
    border-radius: 8px;
    text-align: center;
    background: var(--bg-card);
    border: none;
    cursor: pointer;
    transition: all 150ms ease-out;
    color: inherit;
    user-select: none;
  }

  .card:not(.active):hover {
    background: var(--bg-card-hover);
    transform: scale(1.02);
  }

  .card:not(.active):active {
    transform: scale(0.98);
  }

  .card.active {
    background: var(--bg-card-hover);
    outline: 2px solid rgba(255, 255, 255, 0.4);
    cursor: default;
  }

  .card.dragging {
    opacity: 0.4;
    transform: scale(0.95);
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
    background: var(--bg-muted);
    transition: background 150ms;
    pointer-events: none;
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
    color: var(--fg);
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
    border: 2px solid var(--bg-elevated);
    border-top-color: var(--fg);
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
    background: var(--bg-elevated);
  }

  .avatar.active {
    outline: 2px solid rgba(255, 255, 255, 0.2);
  }

  .name {
    font-size: 12px;
    font-weight: 500;
    color: var(--fg);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    pointer-events: none;
  }

  .username {
    font-size: 10px;
    color: var(--fg-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    pointer-events: none;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
