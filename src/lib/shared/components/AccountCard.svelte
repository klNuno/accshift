<script lang="ts">
  import { onMount } from "svelte";
  import type { PlatformAccount } from "../platform";
  import { formatRelativeTimeCompact } from "$lib/shared/time";

  import type { BanInfo } from "$lib/features/steam/types";

  let {
    account,
    isActive,
    onSwitch,
    onContextMenu,
    isDragged = false,
    avatarUrl = null,
    isLoadingAvatar = false,
    isRefreshingAvatar = false,
    banInfo = undefined,
    cardColor = "",
    showUsername = true,
    showLastLogin = false,
    lastLoginAt = null,
  }: {
    account: PlatformAccount;
    isActive: boolean;
    onSwitch: () => void;
    onContextMenu: (e: MouseEvent) => void;
    isDragged?: boolean;
    avatarUrl?: string | null;
    isLoadingAvatar?: boolean;
    isRefreshingAvatar?: boolean;
    banInfo?: BanInfo;
    cardColor?: string;
    showUsername?: boolean;
    showLastLogin?: boolean;
    lastLoginAt?: number | null;
  } = $props();

  let showConfirm = $state(false);
  let cardRef = $state<HTMLDivElement | null>(null);
  let nameContainerRef = $state<HTMLDivElement | null>(null);
  let nameRef = $state<HTMLSpanElement | null>(null);
  let isOverflowing = $state(false);
  let marqueeShiftPx = $state(0);

  // Ban outline color
  let banOutlineColor = $derived.by(() => {
    if (!banInfo) return "";
    if (banInfo.vac_banned || banInfo.number_of_game_bans > 0) return "rgba(239, 68, 68, 0.6)";
    if (banInfo.community_banned || (banInfo.economy_ban && banInfo.economy_ban !== "none")) return "rgba(234, 179, 8, 0.6)";
    return "";
  });

  onMount(() => {
    function onDocClick(e: MouseEvent) {
      if (showConfirm && cardRef && !cardRef.contains(e.target as Node)) {
        showConfirm = false;
      }
    }
    document.addEventListener("mousedown", onDocClick);
    window.addEventListener("resize", checkOverflow);
    checkOverflow();
    return () => {
      document.removeEventListener("mousedown", onDocClick);
      window.removeEventListener("resize", checkOverflow);
    };
  });

  function checkOverflow() {
    if (nameRef && nameContainerRef) {
      const overflowPx = Math.max(0, nameRef.scrollWidth - nameContainerRef.clientWidth);
      marqueeShiftPx = overflowPx;
      isOverflowing = overflowPx > 2;
    }
  }

  $effect(() => {
    // Re-check overflow when displayName changes
    account.displayName;
    account.username;
    setTimeout(checkOverflow, 0);
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
  style={cardColor ? `--card-custom-color: ${cardColor};` : ""}
  class="card"
  class:custom-color={!!cardColor}
  class:active={isActive}
  class:dragging={isDragged}
  class:ban-red={banOutlineColor.includes("239")}
  class:ban-yellow={banOutlineColor.includes("234")}
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
        <svg width="24" height="24" viewBox="0 0 24 24" fill="var(--fg)">
          <path d="M8 5v14l11-7z" />
        </svg>
      </div>
    {/if}
  </div>

  <div class="name" bind:this={nameContainerRef} class:marquee={isOverflowing} style={`--marquee-shift:${marqueeShiftPx}px;`}>
    <span bind:this={nameRef} class="name-inner">
      {account.displayName || account.username}
    </span>
  </div>

  {#if showUsername}
    <div class="username">{account.username}</div>
  {/if}
  {#if showLastLogin}
    <div class="last-login">{formatRelativeTimeCompact(lastLoginAt)}</div>
  {/if}

  {#if banInfo}
    <div class="ban-badges">
      {#if banInfo.vac_banned}
        <span class="ban-badge vac">VAC</span>
      {/if}
      {#if banInfo.community_banned}
        <span class="ban-badge community">BANNED</span>
      {/if}
      {#if banInfo.number_of_game_bans > 0}
        <span class="ban-badge game">GAME BAN</span>
      {/if}
    </div>
  {/if}
</div>

<style>
  .card {
    width: 100px;
    padding: 8px;
    box-sizing: border-box;
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

  .card.custom-color {
    background: color-mix(in srgb, var(--card-custom-color) 24%, var(--bg-card));
    outline: 1px solid color-mix(in srgb, var(--card-custom-color) 55%, transparent);
  }

  .card.custom-color:not(.active):hover {
    background: color-mix(in srgb, var(--card-custom-color) 32%, var(--bg-card-hover));
  }

  .card:not(.active):active {
    transform: scale(0.98);
  }

  .card.active {
    background: var(--bg-card-hover);
    outline: 2px solid rgba(255, 255, 255, 0.4);
    cursor: default;
  }

  .card.ban-red {
    outline: 2px solid rgba(239, 68, 68, 0.6);
  }

  .card.ban-yellow:not(.ban-red) {
    outline: 2px solid rgba(234, 179, 8, 0.6);
  }

  .card.active.ban-red {
    outline: 2px solid rgba(239, 68, 68, 0.6);
  }

  .card.active.ban-yellow:not(.ban-red) {
    outline: 2px solid rgba(234, 179, 8, 0.6);
  }

  .card.dragging {
    opacity: 0.4;
    transform: scale(0.95);
  }

  .avatar {
    position: relative;
    width: 68px;
    height: 68px;
    margin: 0 auto 8px;
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
    font-size: 20px;
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
    overflow: hidden;
    white-space: nowrap;
    pointer-events: none;
  }

  .name-inner {
    display: inline-block;
    font-size: 12px;
    font-weight: 500;
    color: var(--fg);
    white-space: nowrap;
  }

  .name:not(.marquee) .name-inner {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .card:hover .name.marquee .name-inner {
    animation: marquee 1.6s linear infinite;
  }

  @keyframes marquee {
    0% { transform: translateX(0); }
    10% { transform: translateX(0); }
    90% { transform: translateX(calc(-1 * var(--marquee-shift, 0px))); }
    100% { transform: translateX(calc(-1 * var(--marquee-shift, 0px))); }
  }

  .username {
    font-size: 10px;
    color: var(--fg-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    pointer-events: none;
  }

  .last-login {
    margin-top: 1px;
    font-size: 9px;
    color: var(--fg-subtle);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    pointer-events: none;
  }

  .ban-badges {
    display: flex;
    justify-content: center;
    gap: 3px;
    margin-top: 2px;
    flex-wrap: wrap;
    pointer-events: none;
  }

  .ban-badge {
    font-size: 8px;
    font-weight: 700;
    letter-spacing: 0.3px;
    padding: 1px 4px;
    border-radius: 3px;
    line-height: 1.2;
    text-transform: uppercase;
  }

  .ban-badge.vac {
    background: rgba(239, 68, 68, 0.2);
    color: #f87171;
  }

  .ban-badge.community {
    background: rgba(239, 68, 68, 0.2);
    color: #f87171;
  }

  .ban-badge.game {
    background: rgba(251, 146, 60, 0.2);
    color: #fb923c;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
