<script lang="ts">
  import type { PlatformAccount } from "../platform";
  import type { FolderInfo } from "../../features/folders/types";
  import type { BanInfo } from "../../platforms/steam/types";
  import { formatRelativeTimeCompact } from "$lib/shared/time";

  let {
    account = null,
    folder = null,
    isBack = false,
    isActive = false,
    isSelected = false,
    isDragged = false,
    isDragOver = false,
    avatarUrl = null,
    banInfo = undefined,
    showUsername = true,
    showLastLogin = false,
    lastLoginAt = null,
    onClick,
    onContextMenu = (_e: MouseEvent) => {},
    onDblClick = () => {},
  }: {
    account?: PlatformAccount | null;
    folder?: FolderInfo | null;
    isBack?: boolean;
    isActive?: boolean;
    isSelected?: boolean;
    isDragged?: boolean;
    isDragOver?: boolean;
    avatarUrl?: string | null;
    banInfo?: BanInfo;
    showUsername?: boolean;
    showLastLogin?: boolean;
    lastLoginAt?: number | null;
    onClick: () => void;
    onContextMenu?: (e: MouseEvent) => void;
    onDblClick?: () => void;
  } = $props();

  function getInitials(name: string): string {
    return name.slice(0, 2).toUpperCase();
  }

  function handleContextMenu(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    onContextMenu(e);
  }

  let hasRedWarning = $derived.by(() =>
    Boolean(banInfo && (banInfo.vac_banned || banInfo.number_of_game_bans > 0))
  );

  let hasOrangeWarning = $derived.by(() =>
    Boolean(banInfo && banInfo.community_banned)
  );

  let banHoverMessage = $derived.by(() => {
    if (!banInfo) return "";
    const lines: string[] = [];
    if (banInfo.community_banned) {
      lines.push("Community ban");
    }
    if (banInfo.vac_banned) {
      const vacCount = Math.max(1, banInfo.number_of_vac_bans || 0);
      lines.push(`${vacCount} VAC ban${vacCount > 1 ? "s" : ""}`);
    }
    if (banInfo.number_of_game_bans > 0) {
      lines.push(`${banInfo.number_of_game_bans} game ban${banInfo.number_of_game_bans > 1 ? "s" : ""}`);
    }
    return lines.join("\n");
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="row"
  class:selected={isSelected}
  class:active={isActive}
  class:dragging={isDragged}
  class:drag-over={isDragOver}
  class:ban-red={hasRedWarning}
  class:ban-orange={hasOrangeWarning}
  onclick={onClick}
  ondblclick={onDblClick}
  oncontextmenu={handleContextMenu}
  data-account-id={account?.id}
  data-folder-id={folder?.id}
  data-back-card={isBack ? "true" : undefined}
  title={account && banHoverMessage ? banHoverMessage : undefined}
>
  {#if isBack}
    <div class="icon back-icon">
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M19 12H5" />
        <path d="M12 19l-7-7 7-7" />
      </svg>
    </div>
    <div class="info">
      <span class="name-text">Back</span>
    </div>
  {:else if folder}
    <div class="icon folder-icon">
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
        <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
      </svg>
    </div>
    <div class="info">
      <span class="name-text">{folder.name}</span>
    </div>
  {:else if account}
    <div class="avatar" class:ban-red={hasRedWarning} class:ban-orange={hasOrangeWarning}>
      {#if avatarUrl}
        <img src={avatarUrl} alt={account.displayName} draggable={false} />
      {:else}
        <span class="initials">{getInitials(account.displayName || account.username)}</span>
      {/if}
    </div>
    <div class="info">
      <span class="name-text">{account.displayName || account.username}</span>
      {#if showUsername || showLastLogin}
        <span class="meta-stack">
          {#if showUsername}
            <span class="username-text">{account.username}</span>
          {/if}
          {#if showLastLogin}
            <span class="meta-text">{formatRelativeTimeCompact(lastLoginAt)}</span>
          {/if}
        </span>
      {/if}
    </div>
    {#if hasRedWarning || hasOrangeWarning}
      <div class="warnings">
        {#if hasRedWarning}
          <span class="warning warning-red">!</span>
        {/if}
        {#if hasOrangeWarning}
          <span class="warning warning-orange">!</span>
        {/if}
      </div>
    {/if}
    {#if isActive}
      <div class="active-badge">Active</div>
    {/if}
  {/if}
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 10px;
    border-radius: 6px;
    cursor: pointer;
    transition: background 100ms;
    user-select: none;
    border: 1px solid transparent;
  }

  .row:hover {
    background: var(--bg-card-hover);
  }

  .row.selected {
    background: var(--bg-card);
    border-color: rgba(255, 255, 255, 0.1);
  }

  .row.active {
    background: var(--bg-card-hover);
  }

  .row.dragging {
    opacity: 0.4;
  }

  .row.drag-over {
    border-color: #3b82f6;
    background: rgba(59, 130, 246, 0.1);
  }

  .row.ban-red {
    border-color: rgba(239, 68, 68, 0.45);
  }

  .row.ban-orange:not(.ban-red) {
    border-color: rgba(234, 179, 8, 0.45);
  }

  .avatar {
    width: 32px;
    height: 32px;
    border-radius: 4px;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-muted);
    flex-shrink: 0;
    pointer-events: none;
  }

  .avatar.ban-red {
    outline: 2px solid rgba(239, 68, 68, 0.65);
  }

  .avatar.ban-orange {
    box-shadow: 0 0 0 2px rgba(234, 179, 8, 0.7);
  }

  .avatar.ban-red.ban-orange {
    box-shadow: 0 0 0 4px rgba(234, 179, 8, 0.45);
  }

  .avatar img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    -webkit-user-drag: none;
    user-select: none;
  }

  .avatar .initials {
    font-size: 12px;
    font-weight: 600;
    color: var(--fg);
  }

  .icon {
    width: 32px;
    height: 32px;
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-muted);
    color: var(--fg-muted);
    flex-shrink: 0;
    pointer-events: none;
  }

  .info {
    display: flex;
    flex-direction: column;
    min-width: 0;
    gap: 0;
    pointer-events: none;
  }

  .name-text {
    font-size: 12px;
    font-weight: 500;
    color: var(--fg);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .meta-stack {
    display: flex;
    flex-direction: column;
    gap: 0;
    font-size: 10px;
    line-height: 1.2;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .username-text {
    color: var(--fg-muted);
  }

  .meta-text {
    font-weight: 500;
    color: color-mix(in srgb, var(--fg-subtle) 40%, var(--fg) 60%);
  }

  .active-badge {
    margin-left: 6px;
    font-size: 9px;
    font-weight: 600;
    text-transform: uppercase;
    color: rgba(255, 255, 255, 0.5);
    letter-spacing: 0.5px;
    flex-shrink: 0;
    pointer-events: none;
  }

  .warnings {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 4px;
    pointer-events: none;
  }

  .warning {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 10px;
    font-weight: 800;
    line-height: 1;
  }

  .warning-red {
    background: rgba(239, 68, 68, 0.24);
    color: #fca5a5;
    border: 1px solid rgba(239, 68, 68, 0.55);
  }

  .warning-orange {
    background: rgba(251, 146, 60, 0.24);
    color: #fdba74;
    border: 1px solid rgba(251, 146, 60, 0.55);
  }
</style>
