<script lang="ts">
  import { onMount } from "svelte";
  import type { PlatformAccount } from "../platform";
  import type { AccountWarningPresentation } from "../accountWarnings";
  import type { CardExtensionContent } from "$lib/shared/cardExtension";
  import { hasCardExtensionContent } from "$lib/shared/cardExtension";
  import CardExtensionPanel from "./CardExtensionPanel.svelte";
  import { formatRelativeTimeCompact } from "$lib/shared/time";
  import { getAvatarGradientStyle, getAvatarInitials, getAvatarSeed } from "$lib/shared/avatarFallback";
  import { DEFAULT_LOCALE, translate, type Locale, type MessageKey } from "$lib/i18n";

  let {
    account,
    isActive,
    onSwitch,
    onContextMenu,
    onActivate = () => {},
    isDragged = false,
    avatarUrl = null,
    isLoadingAvatar = false,
    isRefreshingAvatar = false,
    warningInfo = undefined,
    extensionContent = null,
    forceExtensionOpen = false,
    disableExtension = false,
    disableHoverExtension = false,
    cardColor = "",
    showUsername = true,
    showNoteInline = false,
    showLastLogin = false,
    lastLoginUnknownKey = "time.unknown",
    lastLoginAt = null,
    note = "",
    locale = DEFAULT_LOCALE,
  }: {
    account: PlatformAccount;
    isActive: boolean;
    onSwitch: () => void;
    onContextMenu: (e: MouseEvent) => void;
    onActivate?: () => void;
    isDragged?: boolean;
    avatarUrl?: string | null;
    isLoadingAvatar?: boolean;
    isRefreshingAvatar?: boolean;
    warningInfo?: AccountWarningPresentation;
    extensionContent?: CardExtensionContent | null;
    forceExtensionOpen?: boolean;
    disableExtension?: boolean;
    disableHoverExtension?: boolean;
    cardColor?: string;
    showUsername?: boolean;
    showNoteInline?: boolean;
    showLastLogin?: boolean;
    lastLoginUnknownKey?: MessageKey;
    lastLoginAt?: number | null;
    note?: string;
    locale?: Locale;
  } = $props();

  let showConfirm = $state(false);
  let cardRef = $state<HTMLDivElement | null>(null);
  let nameContainerRef = $state<HTMLDivElement | null>(null);
  let nameRef = $state<HTMLSpanElement | null>(null);
  let isOverflowing = $state(false);
  let isHovered = $state(false);
  let panelSide = $state<"left" | "right">("right");
  let marqueeShiftPx = $state(0);
  let marqueeMoveDurationMs = $state(0);
  let marqueeOffsetPx = $state(0);
  let marqueePauseTimer: ReturnType<typeof setTimeout> | null = null;
  let marqueeDirection = $state<"to-end" | "to-start">("to-end");

  const MARQUEE_SPEED_PX_PER_SEC = 42;
  const MARQUEE_PAUSE_MS = 2000;
  const EXTENSION_WIDTH_PX = 176;
  const EXTENSION_OVERLAP_PX = 18;
  const EXTENSION_VIEWPORT_GAP_PX = 12;
  const noteText = $derived(note.trim());
  const hasUsername = $derived(Boolean(showUsername && account.username.trim()));
  const avatarSeed = $derived(getAvatarSeed(account.displayName, account.username, account.id));
  const hasRedWarning = $derived(warningInfo?.cardOutlineTone === "red");
  const hasOrangeWarning = $derived(warningInfo?.cardOutlineTone === "orange");
  const hasInlineNote = $derived(Boolean(showNoteInline && noteText));
  const hasExtension = $derived(hasCardExtensionContent(extensionContent));
  const isExtensionVisible = $derived(
    Boolean(hasExtension && !isDragged && !disableExtension && (forceExtensionOpen || (!disableHoverExtension && isHovered)))
  );

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

  function updatePanelSide() {
    if (!cardRef) return;
    const rect = cardRef.getBoundingClientRect();
    const roomOnRight = window.innerWidth - rect.right;
    const needed = EXTENSION_WIDTH_PX - EXTENSION_OVERLAP_PX + EXTENSION_VIEWPORT_GAP_PX;
    panelSide = roomOnRight >= needed ? "right" : "left";
  }

  function handleMouseEnter() {
    isHovered = true;
    updatePanelSide();
    startMarquee();
  }

  function handleMouseLeave() {
    isHovered = false;
    stopMarquee();
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
    account.displayName;
    account.username;
    setTimeout(checkOverflow, 0);
  });

  $effect(() => {
    if (isDragged) showConfirm = false;
  });

  $effect(() => {
    if (forceExtensionOpen) {
      updatePanelSide();
    }
  });

  function handleClick() {
    if (isDragged) return;
    onActivate();
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
    onActivate();
    showConfirm = false;
    onContextMenu(e);
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="card-shell"
  class:extension-visible={isExtensionVisible}
  onmouseenter={handleMouseEnter}
  onmouseleave={handleMouseLeave}
>
  {#if isExtensionVisible && extensionContent}
    <CardExtensionPanel content={extensionContent} side={panelSide} />
  {/if}

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
    class:ban-red={hasRedWarning}
    class:ban-yellow={hasOrangeWarning}
  >
    <div
      class="avatar"
      class:active={isActive}
      style={!avatarUrl && !isLoadingAvatar ? getAvatarGradientStyle(avatarSeed) : ""}
    >
      <div class="avatar-media">
        {#if isLoadingAvatar}
          <div class="loader-anchor">
            <div class="loader"></div>
          </div>
        {:else if avatarUrl}
          <img
            src={avatarUrl}
            alt={account.displayName}
            draggable={false}
            class:blurred={isRefreshingAvatar || showConfirm}
          />
          {#if isRefreshingAvatar}
            <div class="loader-anchor">
              <div class="loader"></div>
            </div>
          {/if}
        {:else}
          <span class="initials" class:blurred-text={showConfirm}>
            {getAvatarInitials(account.displayName || account.username)}
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

    {#if hasUsername || hasInlineNote || showLastLogin}
      <div class="meta-stack">
        {#if hasUsername}
          <div class="username">
            {#if noteText && !showNoteInline}
              <span class="note-info-icon" aria-label={translate(locale, "card.noteAttached")}>i</span>
            {/if}
            <span class="username-text">{account.username}</span>
          </div>
        {/if}
        {#if hasInlineNote}
          <div class="note">{noteText}</div>
        {/if}
        {#if showLastLogin}
          <div class="last-login">{formatRelativeTimeCompact(lastLoginAt, locale, lastLoginUnknownKey)}</div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .card-shell {
    position: relative;
    overflow: visible;
    isolation: isolate;
  }

  .card-shell.extension-visible {
    z-index: 24;
  }

  .card {
    position: relative;
    z-index: 2;
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

  .loader-anchor {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    display: flex;
    align-items: center;
    justify-content: center;
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
