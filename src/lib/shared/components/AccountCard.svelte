<script lang="ts">
  import { onDestroy } from "svelte";
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
    entranceDelay = 0,
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
    entranceDelay?: number;
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
  let overflowCheckFrame: number | null = null;
  let marqueeDirection = $state<"to-end" | "to-start">("to-end");

  const MARQUEE_SPEED_PX_PER_SEC = 42;
  const MARQUEE_PAUSE_MS = 2000;
  const EXTENSION_DETAIL_WIDTH_PX = 156;
  const EXTENSION_VIEWPORT_GAP_PX = 12;
  const noteText = $derived(note.trim());
  const hasUsername = $derived(Boolean(showUsername && account.username.trim()));
  const hasRedWarning = $derived(warningInfo?.cardOutlineTone === "red");
  const hasOrangeWarning = $derived(warningInfo?.cardOutlineTone === "orange");
  const hasInlineNote = $derived(Boolean(showNoteInline && noteText));
  const hasExtension = $derived(hasCardExtensionContent(extensionContent));
  const isExtensionVisible = $derived(
    Boolean(hasExtension && !isDragged && !disableExtension && (forceExtensionOpen || (!disableHoverExtension && isHovered)))
  );
  const avatarSeed = $derived(getAvatarSeed(account.displayName || "", account.username || "", account.id));

  onDestroy(() => {
    clearMarqueePauseTimer();
    if (overflowCheckFrame !== null) {
      cancelAnimationFrame(overflowCheckFrame);
      overflowCheckFrame = null;
    }
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

  function queueOverflowCheck() {
    if (overflowCheckFrame !== null) cancelAnimationFrame(overflowCheckFrame);
    overflowCheckFrame = requestAnimationFrame(() => {
      overflowCheckFrame = null;
      checkOverflow();
    });
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
    const needed = EXTENSION_DETAIL_WIDTH_PX + EXTENSION_VIEWPORT_GAP_PX;
    panelSide = roomOnRight >= needed ? "right" : "left";
  }

  function handleMouseEnter() {
    isHovered = true;
    updatePanelSide();
    queueOverflowCheck();
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
    if (isHovered) {
      queueOverflowCheck();
      return;
    }
    isOverflowing = false;
    marqueeShiftPx = 0;
    marqueeMoveDurationMs = 0;
    marqueeOffsetPx = 0;
  });

  $effect(() => {
    if (isDragged) showConfirm = false;
  });

  $effect(() => {
    if (forceExtensionOpen) {
      updatePanelSide();
    }
  });

  $effect(() => {
    if (!showConfirm || !cardRef || typeof document === "undefined") return;
    function onDocClick(e: MouseEvent) {
      if (cardRef && !cardRef.contains(e.target as Node)) {
        showConfirm = false;
      }
    }
    document.addEventListener("mousedown", onDocClick);
    return () => {
      document.removeEventListener("mousedown", onDocClick);
    };
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
  style={cardColor ? `--card-custom-color: ${cardColor};` : ""}
  onmouseenter={handleMouseEnter}
  onmouseleave={handleMouseLeave}
>
  {#if hasExtension && extensionContent}
    <div
      class="extension-hitbox"
      class:visible={isExtensionVisible}
      class:left={panelSide === "left"}
      class:right={panelSide === "right"}
      aria-hidden={!isExtensionVisible}
    >
      <div
        class="extension-surface"
        class:visible={isExtensionVisible}
        class:active={isActive}
        class:custom-color={!!cardColor}
        class:ban-red={hasRedWarning}
        class:ban-yellow={hasOrangeWarning}
        class:left={panelSide === "left"}
        class:right={panelSide === "right"}
      >
        <div class="details-wrap">
          <CardExtensionPanel content={extensionContent} />
        </div>
      </div>
    </div>
  {/if}

  <div
    bind:this={cardRef}
    onclick={handleClick}
    oncontextmenu={handleContextMenu}
    data-account-id={account.id}
    class="card"
    class:custom-color={!!cardColor}
    class:active={isActive}
    class:dragging={isDragged}
    class:ban-red={hasRedWarning}
    class:ban-yellow={hasOrangeWarning}
    class:entrance={entranceDelay >= 0}
    style:--entrance-delay={`${entranceDelay}ms`}
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
    width: var(--grid-card-width);
    min-width: var(--grid-card-width);
    overflow: visible;
    isolation: isolate;
    flex: 0 0 auto;
  }

  .card-shell.extension-visible {
    z-index: 24;
  }

  .extension-hitbox {
    position: absolute;
    top: -2px;
    bottom: -2px;
    width: calc(var(--grid-card-width) + 156px);
    pointer-events: none;
    z-index: 1;
  }

  .extension-hitbox.right {
    left: 0;
  }

  .extension-hitbox.left {
    right: 0;
  }

  .extension-surface {
    position: absolute;
    inset: 0;
    border-radius: var(--grid-card-radius);
    background: var(--bg-overlay);
    opacity: 0;
    transform: translateY(2px) scaleX(0.96);
    transition:
      opacity 140ms ease-out,
      transform 220ms cubic-bezier(0.22, 1, 0.36, 1),
      box-shadow 180ms ease-out,
      background 180ms ease-out,
      outline-color 120ms ease-out;
    box-shadow: none;
    overflow: hidden;
    pointer-events: none;
  }

  .extension-surface.right {
    transform-origin: left center;
  }

  .extension-surface.left {
    transform-origin: right center;
  }

  .extension-surface.visible {
    opacity: 1;
    transform: translateY(0) scaleX(1);
    box-shadow: 0 14px 28px rgba(0, 0, 0, 0.18);
  }

  .card-shell:hover .extension-surface.visible {
    transform: translateY(-2px) scaleX(1);
    box-shadow: 0 18px 32px rgba(0, 0, 0, 0.2);
  }

  .extension-surface.custom-color {
    background: color-mix(in srgb, var(--card-custom-color) 18%, var(--bg-overlay));
  }

  .extension-surface.active.visible {
    box-shadow:
      0 0 0 2px rgba(255, 255, 255, 0.62),
      0 0 0 4px rgba(9, 9, 11, 0.45),
      0 14px 28px rgba(0, 0, 0, 0.18);
  }

  .extension-surface.custom-color.active.visible {
    box-shadow:
      0 0 0 2px rgba(255, 255, 255, 0.72),
      0 0 0 5px color-mix(in srgb, var(--card-custom-color) 50%, rgba(9, 9, 11, 0.62)),
      0 14px 28px rgba(0, 0, 0, 0.18);
  }

  .extension-surface.ban-red.visible {
    outline: 2px solid rgba(239, 68, 68, 0.6);
  }

  .extension-surface.ban-yellow.visible:not(.ban-red) {
    outline: 2px solid rgba(234, 179, 8, 0.6);
  }

  .details-wrap {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 156px;
    box-sizing: border-box;
  }

  .extension-surface.right .details-wrap {
    right: 0;
    border-left: 1px solid color-mix(in srgb, var(--border) 82%, transparent);
  }

  .extension-surface.left .details-wrap {
    left: 0;
    border-right: 1px solid color-mix(in srgb, var(--border) 82%, transparent);
  }

  .card {
    position: relative;
    z-index: 2;
    width: var(--grid-card-width);
    min-height: var(--grid-card-min-height);
    padding: var(--grid-card-padding);
    box-sizing: border-box;
    border-radius: var(--grid-card-radius);
    text-align: center;
    background: var(--bg-card);
    border: none;
    cursor: pointer;
    transition: background 180ms ease-out, transform 180ms ease-out, box-shadow 180ms ease-out, outline-color 120ms ease-out;
    color: inherit;
    user-select: none;
  }

  .card.entrance {
    animation: cardEntrance 200ms ease-out backwards;
    animation-delay: var(--entrance-delay, 0ms);
  }

  @keyframes cardEntrance {
    from { opacity: 0; transform: translateY(8px) scale(0.97); }
    to   { opacity: 1; transform: translateY(0) scale(1); }
  }

  @media (prefers-reduced-motion: reduce) {
    .card.entrance {
      animation: none;
    }
  }

  .card:not(.active):hover {
    background: var(--bg-card-hover);
    transform: translateY(-2px);
    box-shadow: 0 12px 24px rgba(0, 0, 0, 0.18);
  }

  .card-shell:hover .card:not(.active):not(.dragging) {
    background: var(--bg-card-hover);
    transform: translateY(-2px);
    box-shadow: 0 12px 24px rgba(0, 0, 0, 0.18);
  }

  .card.custom-color {
    background: color-mix(in srgb, var(--card-custom-color) 24%, var(--bg-card));
    outline: 1px solid color-mix(in srgb, var(--card-custom-color) 55%, transparent);
  }

  .card.custom-color:not(.active):hover {
    background: color-mix(in srgb, var(--card-custom-color) 32%, var(--bg-card-hover));
  }

  .card-shell:hover .card.custom-color:not(.active):not(.dragging) {
    background: color-mix(in srgb, var(--card-custom-color) 32%, var(--bg-card-hover));
  }

  .card:not(.active):active {
    transform: translateY(0) scale(0.985);
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
    width: var(--grid-card-avatar-size);
    height: var(--grid-card-avatar-size);
    margin: 0 auto 8px;
    border-radius: 6px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-muted);
    transition: background 150ms, transform 180ms ease-out;
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
    transition: transform 220ms ease-out, filter 300ms ease-out;
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
    transform: translateY(-1px);
  }

  .card:not(.active):hover .avatar-media img {
    transform: scale(1.04);
  }

  .card-shell:hover .card:not(.active):not(.dragging) .avatar {
    background: var(--bg-elevated);
    transform: translateY(-1px);
  }

  .card-shell:hover .card:not(.active):not(.dragging) .avatar-media img {
    transform: scale(1.04);
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
