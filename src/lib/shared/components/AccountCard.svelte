<script lang="ts">
  import { onMount, tick } from "svelte";
  import type { PlatformAccount } from "../platform";
  import { formatRelativeTimeCompact } from "$lib/shared/time";

  import type { BanInfo } from "$lib/platforms/steam/types";

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
    showNoteInline = false,
    showLastLogin = false,
    lastLoginAt = null,
    note = "",
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
    showNoteInline?: boolean;
    showLastLogin?: boolean;
    lastLoginAt?: number | null;
    note?: string;
  } = $props();

  let showConfirm = $state(false);
  let cardRef = $state<HTMLDivElement | null>(null);
  let tooltipRef = $state<HTMLDivElement | null>(null);
  let nameContainerRef = $state<HTMLDivElement | null>(null);
  let nameRef = $state<HTMLSpanElement | null>(null);
  let isOverflowing = $state(false);
  let isHovered = $state(false);
  let marqueeShiftPx = $state(0);
  let marqueeMoveDurationMs = $state(0);
  let marqueeOffsetPx = $state(0);
  let marqueePauseTimer: ReturnType<typeof setTimeout> | null = null;
  let marqueeDirection = $state<"to-end" | "to-start">("to-end");
  let noteTooltipStyle = $state("");

  const MARQUEE_SPEED_PX_PER_SEC = 42;
  const MARQUEE_PAUSE_MS = 2000;
  const TOOLTIP_GAP_PX = 6;
  const VIEWPORT_EDGE_GAP_PX = 4;
  const noteText = $derived(note.trim());

  // Visual severity hint for ban state.
  let banOutlineColor = $derived.by(() => {
    if (!banInfo) return "";
    if (banInfo.vac_banned || banInfo.number_of_game_bans > 0) return "rgba(239, 68, 68, 0.6)";
    if (banInfo.community_banned || (banInfo.economy_ban && banInfo.economy_ban !== "none")) return "rgba(234, 179, 8, 0.6)";
    return "";
  });
  let hasInlineNote = $derived(Boolean(showNoteInline && noteText));
  let showNoteTooltip = $derived(Boolean(isHovered && !isDragged && noteText && !showNoteInline));

  onMount(() => {
    function onDocClick(e: MouseEvent) {
      if (showConfirm && cardRef && !cardRef.contains(e.target as Node)) {
        showConfirm = false;
      }
    }
    document.addEventListener("mousedown", onDocClick);
    window.addEventListener("resize", checkOverflow);
    window.addEventListener("resize", handleViewportChange);
    window.addEventListener("scroll", handleViewportChange, true);
    checkOverflow();
    return () => {
      document.removeEventListener("mousedown", onDocClick);
      window.removeEventListener("resize", checkOverflow);
      window.removeEventListener("resize", handleViewportChange);
      window.removeEventListener("scroll", handleViewportChange, true);
      clearMarqueePauseTimer();
    };
  });

  function clearMarqueePauseTimer() {
    if (!marqueePauseTimer) return;
    clearTimeout(marqueePauseTimer);
    marqueePauseTimer = null;
  }

  function stopMarquee() {
    clearMarqueePauseTimer();
    marqueeOffsetPx = 0;
    marqueeDirection = "to-end";
  }

  function stepMarquee() {
    if (!isHovered || !isOverflowing || marqueeMoveDurationMs <= 0) return;
    marqueeOffsetPx = marqueeDirection === "to-end" ? -marqueeShiftPx : 0;
  }

  function startMarquee() {
    stopMarquee();
    if (!isOverflowing || marqueeMoveDurationMs <= 0) return;
    marqueeDirection = "to-end";
    requestAnimationFrame(() => {
      if (!isHovered || !isOverflowing) return;
      stepMarquee();
    });
  }

  function handleNameTransitionEnd(e: TransitionEvent) {
    if (e.propertyName !== "transform") return;
    if (!isHovered || !isOverflowing || marqueeMoveDurationMs <= 0) return;

    clearMarqueePauseTimer();
    marqueePauseTimer = setTimeout(() => {
      if (!isHovered || !isOverflowing || marqueeMoveDurationMs <= 0) return;
      marqueeDirection = marqueeDirection === "to-end" ? "to-start" : "to-end";
      stepMarquee();
    }, MARQUEE_PAUSE_MS);
  }

  function handleMouseEnter() {
    isHovered = true;
    startMarquee();
    void positionNoteTooltip();
  }

  function handleMouseLeave() {
    isHovered = false;
    stopMarquee();
  }

  function handleViewportChange() {
    if (!showNoteTooltip) return;
    void positionNoteTooltip();
  }

  async function positionNoteTooltip() {
    if (!showNoteTooltip) return;
    await tick();
    if (!cardRef || !tooltipRef) return;

    const cardRect = cardRef.getBoundingClientRect();
    const tooltipRect = tooltipRef.getBoundingClientRect();
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;

    let leftInViewport = cardRect.right - tooltipRect.width;
    leftInViewport = Math.max(VIEWPORT_EDGE_GAP_PX, leftInViewport);
    leftInViewport = Math.min(leftInViewport, viewportWidth - tooltipRect.width - VIEWPORT_EDGE_GAP_PX);

    let topInViewport = cardRect.bottom + TOOLTIP_GAP_PX;
    if (topInViewport + tooltipRect.height > viewportHeight - VIEWPORT_EDGE_GAP_PX) {
      topInViewport = cardRect.top - tooltipRect.height - TOOLTIP_GAP_PX;
    }
    topInViewport = Math.max(VIEWPORT_EDGE_GAP_PX, topInViewport);

    noteTooltipStyle = `left:${leftInViewport - cardRect.left}px;top:${topInViewport - cardRect.top}px;`;
  }

  function checkOverflow() {
    if (nameRef && nameContainerRef) {
      const overflowPx = Math.max(0, nameRef.scrollWidth - nameContainerRef.clientWidth);
      marqueeShiftPx = overflowPx;
      isOverflowing = overflowPx > 2;
      marqueeMoveDurationMs = overflowPx > 0
        ? Math.round((overflowPx / MARQUEE_SPEED_PX_PER_SEC) * 1000)
        : 0;
      if (!isOverflowing) {
        stopMarquee();
      } else if (isHovered) {
        startMarquee();
      }
    }
  }

  $effect(() => {
    // Recalculate marquee width when name fields change.
    account.displayName;
    account.username;
    setTimeout(checkOverflow, 0);
  });

  $effect(() => {
    if (isDragged) showConfirm = false;
  });

  $effect(() => {
    if (!showNoteTooltip) return;
    void positionNoteTooltip();
  });

  function getInitials(name: string): string {
    return name.slice(0, 2).toUpperCase();
  }

  function handleClick() {
    if (isDragged) return;
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

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={cardRef}
  onclick={handleClick}
  oncontextmenu={handleContextMenu}
  onmouseenter={handleMouseEnter}
  onmouseleave={handleMouseLeave}
  data-account-id={account.id}
  style={cardColor ? `--card-custom-color: ${cardColor};` : ""}
  class="card"
  class:custom-color={!!cardColor}
  class:active={isActive}
  class:dragging={isDragged}
  class:ban-red={banOutlineColor.includes("239")}
  class:ban-yellow={banOutlineColor.includes("234")}
>
  {#if showNoteTooltip}
    <div class="note-tooltip" bind:this={tooltipRef} style={noteTooltipStyle} role="tooltip">{noteText}</div>
  {/if}

  <div class="avatar" class:active={isActive}>
    <div class="avatar-media">
      {#if isLoadingAvatar}
        <div class="loader"></div>
      {:else if avatarUrl}
        <img
          src={avatarUrl}
          alt={account.displayName}
          draggable={false}
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
  </div>

  <div
    class="name"
    bind:this={nameContainerRef}
    class:marquee={isOverflowing}
    class:marquee-active={isHovered && isOverflowing}
    style={`--marquee-duration:${marqueeMoveDurationMs}ms;--marquee-offset:${marqueeOffsetPx}px;`}
  >
    <span bind:this={nameRef} class="name-inner" ontransitionend={handleNameTransitionEnd}>
      {account.displayName || account.username}
    </span>
  </div>

  {#if showUsername || hasInlineNote || showLastLogin}
    <div class="meta-stack">
      {#if showUsername}
        <div class="username">
          {#if noteText && !showNoteInline}
            <span class="note-info-icon" aria-label="Note attached">i</span>
          {/if}
          <span class="username-text">{account.username}</span>
        </div>
      {/if}
      {#if hasInlineNote}
        <div class="note">{noteText}</div>
      {/if}
      {#if showLastLogin}
        <div class="last-login">{formatRelativeTimeCompact(lastLoginAt)}</div>
      {/if}
    </div>
  {/if}

</div>

<style>
  .card {
    position: relative;
    overflow: visible;
    z-index: 1;
    width: 100px;
    padding: 8px;
    box-sizing: border-box;
    border-radius: 8px;
    text-align: center;
    background: var(--bg-card);
    border: none;
    cursor: pointer;
    transition: background 150ms ease-out, transform 150ms ease-out, box-shadow 150ms ease-out, outline-color 120ms ease-out;
    color: inherit;
    user-select: none;
  }

  .card:not(.active):hover {
    background: var(--bg-card-hover);
    transform: scale(1.02);
    z-index: 24;
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
    box-shadow:
      0 0 0 2px rgba(255, 255, 255, 0.62),
      0 0 0 4px rgba(9, 9, 11, 0.45);
    cursor: pointer;
    z-index: 18;
  }

  .card.active:not(.custom-color) {
    background: var(--bg-card-hover);
  }

  .card.custom-color.active {
    background: color-mix(in srgb, var(--card-custom-color) 24%, var(--bg-card));
    box-shadow:
      0 0 0 2px rgba(255, 255, 255, 0.72),
      0 0 0 5px color-mix(in srgb, var(--card-custom-color) 50%, rgba(9, 9, 11, 0.62));
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
    z-index: 8;
  }

  .note-tooltip {
    position: absolute;
    top: 0;
    left: 0;
    width: min(190px, calc(100vw - 8px));
    padding: 8px;
    border-radius: 8px;
    border: 1px solid var(--border);
    background: color-mix(in srgb, var(--bg-card) 92%, #000 8%);
    box-shadow: 0 12px 28px rgba(0, 0, 0, 0.45);
    font-size: 10px;
    line-height: 1.3;
    color: var(--fg);
    text-align: left;
    word-break: break-word;
    pointer-events: none;
    z-index: 30;
  }

  .avatar {
    position: relative;
    width: 68px;
    height: 68px;
    margin: 0 auto 8px;
    border-radius: 6px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-muted);
    transition: background 150ms;
    pointer-events: none;
  }

  .avatar-media {
    width: 100%;
    height: 100%;
    border-radius: inherit;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
    position: relative;
  }

  .avatar-media img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    transition: filter 300ms ease-out;
    -webkit-user-drag: none;
    user-select: none;
  }

  .avatar-media img.blurred {
    filter: blur(6px) brightness(0.5);
  }

  .avatar-media .initials {
    font-size: 20px;
    font-weight: 600;
    color: var(--fg);
    transition: filter 300ms ease-out;
  }

  .avatar-media .initials.blurred-text {
    filter: blur(4px) brightness(0.5);
  }

  .play-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    animation: fadeIn 150ms ease-out;
    pointer-events: none;
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
    line-height: 1.2;
    white-space: nowrap;
    transform: translateX(var(--marquee-offset, 0px));
    transition: none;
  }

  .name:not(.marquee) .name-inner {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .name.marquee-active .name-inner {
    transition: transform var(--marquee-duration, 1600ms) linear;
  }

  .note {
    margin-top: 0;
    display: block;
    width: 100%;
    max-width: 100%;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    pointer-events: none;
    font-size: 9px;
    font-weight: 500;
    color: var(--fg);
    line-height: 1.2;
  }

  .meta-stack {
    margin-top: 4px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 10px;
    line-height: 1.2;
    pointer-events: none;
  }

  .username {
    color: var(--fg-muted);
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    width: 100%;
    max-width: 100%;
    min-width: 0;
    overflow: hidden;
  }

  .note-info-icon {
    width: 12px;
    height: 12px;
    border-radius: 999px;
    border: 1px solid rgba(255, 255, 255, 0.92);
    color: #fff;
    font-size: 8px;
    font-weight: 700;
    line-height: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
  }

  .username-text {
    display: block;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .last-login {
    display: block;
    width: 100%;
    max-width: 100%;
    font-size: 10px;
    font-weight: 500;
    color: color-mix(in srgb, var(--fg-subtle) 40%, var(--fg) 60%);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
