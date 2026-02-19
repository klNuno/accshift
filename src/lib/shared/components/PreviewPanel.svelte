<script lang="ts">
  import type { PlatformAccount } from "../platform";
  import type { BanInfo } from "$lib/platforms/steam/types";
  import { formatRelativeTimeCompact } from "$lib/shared/time";

  let {
    account,
    isActive = false,
    avatarUrl = null,
    showUsername = true,
    showLastLogin = false,
    lastLoginAt = null,
    accentColor = "#3b82f6",
    onSwitch,
    banInfo = undefined,
  }: {
    account: PlatformAccount;
    isActive?: boolean;
    avatarUrl?: string | null;
    showUsername?: boolean;
    showLastLogin?: boolean;
    lastLoginAt?: number | null;
    accentColor?: string;
    onSwitch: () => void;
    banInfo?: BanInfo;
  } = $props();

  function getInitials(name: string): string {
    return name.slice(0, 2).toUpperCase();
  }

  type BanWarningTone = "red" | "orange";
  interface BanWarningChip {
    tone: BanWarningTone;
    text: string;
  }

  let banWarnings = $derived.by(() => {
    if (!banInfo) return [] as BanWarningChip[];
    const chips: BanWarningChip[] = [];
    if (banInfo.community_banned) {
      chips.push({ tone: "orange", text: "Community ban" });
    }
    if (banInfo.vac_banned) {
      const vacCount = Math.max(1, banInfo.number_of_vac_bans || 0);
      chips.push({ tone: "red", text: `${vacCount} VAC ban${vacCount > 1 ? "s" : ""}` });
    }
    if (banInfo.number_of_game_bans > 0) {
      chips.push({ tone: "red", text: `${banInfo.number_of_game_bans} game ban${banInfo.number_of_game_bans > 1 ? "s" : ""}` });
    }
    return chips;
  });
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
  {#if showUsername || showLastLogin}
    <div class="meta-stack">
      {#if showUsername}
        <span class="username">{account.username}</span>
      {/if}
      {#if showLastLogin}
        <span class="meta">{formatRelativeTimeCompact(lastLoginAt)}</span>
      {/if}
    </div>
  {/if}

  {#if banWarnings.length > 0}
    <div class="ban-badges">
      {#each banWarnings as warning, index (`${warning.tone}-${warning.text}-${index}`)}
        <span class="ban-badge" class:red={warning.tone === "red"} class:orange={warning.tone === "orange"}>
          {warning.text}
        </span>
      {/each}
    </div>
  {/if}

  {#if isActive}
    <div class="status">Currently active</div>
  {/if}
  <button
    class="switch-btn"
    style="background: {accentColor};"
    onclick={onSwitch}
  >
    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
      <path d="M8 5v14l11-7z" />
    </svg>
    {isActive ? "Switch Again" : "Switch Account"}
  </button>
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

  .meta-stack {
    margin-top: 4px;
    max-width: 100%;
    display: flex;
    flex-direction: column;
    gap: 0;
    font-size: 12px;
    line-height: 1.2;
    text-align: center;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .username {
    color: var(--fg-muted);
  }

  .meta {
    font-weight: 500;
    color: color-mix(in srgb, var(--fg-subtle) 40%, var(--fg) 60%);
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

  .ban-badge.red {
    background: rgba(239, 68, 68, 0.2);
    color: #f87171;
  }

  .ban-badge.orange {
    background: rgba(251, 146, 60, 0.2);
    color: #fb923c;
  }
</style>
