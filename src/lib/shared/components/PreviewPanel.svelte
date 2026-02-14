<script lang="ts">
  import type { PlatformAccount } from "../platform";
  import type { BanInfo } from "$lib/features/steam/types";

  let {
    account,
    isActive = false,
    avatarUrl = null,
    accentColor = "#3b82f6",
    onSwitch,
    banInfo = undefined,
  }: {
    account: PlatformAccount;
    isActive?: boolean;
    avatarUrl?: string | null;
    accentColor?: string;
    onSwitch: () => void;
    banInfo?: BanInfo;
  } = $props();

  function getInitials(name: string): string {
    return name.slice(0, 2).toUpperCase();
  }
</script>

<div class="preview">
  <div class="avatar-large">
    {#if avatarUrl}
      <img src={avatarUrl} alt={account.displayName} />
    {:else}
      <span class="initials">{getInitials(account.displayName || account.username)}</span>
    {/if}
  </div>

  <div class="display-name">{account.displayName || account.username}</div>
  <div class="username">{account.username}</div>

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

  {#if isActive}
    <div class="status">Currently active</div>
  {:else}
    <button
      class="switch-btn"
      style="background: {accentColor};"
      onclick={onSwitch}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
        <path d="M8 5v14l11-7z" />
      </svg>
      Switch Account
    </button>
  {/if}
</div>

<style>
  .preview {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 24px 16px;
    height: 100%;
    box-sizing: border-box;
  }

  .avatar-large {
    width: 120px;
    height: 120px;
    border-radius: 8px;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-muted);
    margin-bottom: 16px;
  }

  .avatar-large img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .avatar-large .initials {
    font-size: 36px;
    font-weight: 600;
    color: var(--fg);
  }

  .display-name {
    font-size: 15px;
    font-weight: 600;
    color: var(--fg);
    text-align: center;
    word-break: break-word;
  }

  .username {
    font-size: 12px;
    color: var(--fg-muted);
    margin-top: 2px;
    text-align: center;
  }

  .status {
    margin-top: 16px;
    font-size: 11px;
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    font-weight: 600;
  }

  .switch-btn {
    margin-top: 16px;
    border: none;
    color: white;
    font-size: 12px;
    font-weight: 500;
    padding: 8px 16px;
    border-radius: 6px;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 6px;
    transition: filter 150ms;
  }

  .switch-btn:hover {
    filter: brightness(1.15);
  }

  .switch-btn:active {
    filter: brightness(0.9);
  }

  .ban-badges {
    display: flex;
    gap: 4px;
    margin-top: 6px;
    flex-wrap: wrap;
    justify-content: center;
  }

  .ban-badge {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.3px;
    padding: 2px 6px;
    border-radius: 3px;
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
</style>
