<script lang="ts">
  import type { ToastType, ToastAction } from "./store.svelte";

  let {
    message,
    durationMs = 3000,
    type = "info",
    toastAction = undefined,
    resetKey = 0,
    dismissLabel = "Dismiss",
    onDone,
  }: {
    message: string;
    durationMs?: number | null;
    type?: ToastType;
    toastAction?: ToastAction;
    resetKey?: number;
    dismissLabel?: string;
    onDone: () => void;
  } = $props();

  let timer: ReturnType<typeof setTimeout> | undefined;
  let remainingMs = 0;
  let startedAt = 0;
  let hovered = false;

  function hasDuration() {
    return durationMs != null && Number.isFinite(durationMs) && durationMs > 0;
  }

  function clearTimer() {
    if (timer !== undefined) {
      clearTimeout(timer);
      timer = undefined;
    }
  }

  function startTimer() {
    startedAt = Date.now();
    timer = setTimeout(onDone, remainingMs);
  }

  // resetKey bumps when the store dedups an identical message: restart the countdown.
  $effect(() => {
    void resetKey;
    void durationMs;
    clearTimer();
    if (!hasDuration()) return;
    remainingMs = durationMs as number;
    if (!hovered) startTimer();
    return clearTimer;
  });

  function handleMouseEnter() {
    hovered = true;
    if (timer !== undefined) {
      clearTimer();
      remainingMs = Math.max(0, remainingMs - (Date.now() - startedAt));
    }
  }

  function handleMouseLeave() {
    hovered = false;
    if (!hasDuration()) return;
    if (remainingMs <= 0) {
      onDone();
      return;
    }
    startTimer();
  }

  function handleAction() {
    toastAction?.action();
    onDone();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions (hover only pauses the auto-dismiss timer) -->
<div
  class="toast"
  class:success={type === "success"}
  class:error={type === "error"}
  onmouseenter={handleMouseEnter}
  onmouseleave={handleMouseLeave}
>
  <div class="toast-body">
    <svg class="toast-icon" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      {#if type === "success"}
        <path d="M20 6L9 17l-5-5" />
      {:else if type === "error"}
        <circle cx="12" cy="12" r="10" />
        <line x1="15" y1="9" x2="9" y2="15" />
        <line x1="9" y1="9" x2="15" y2="15" />
      {:else}
        <circle cx="12" cy="12" r="10" />
        <line x1="12" y1="16" x2="12" y2="12" />
        <line x1="12" y1="8" x2="12.01" y2="8" />
      {/if}
    </svg>
    <span class="toast-text" title={message}>{message}</span>
    {#if toastAction}
      <button class="toast-action" onclick={handleAction}>{toastAction.label}</button>
    {/if}
    <button class="toast-close" onclick={onDone} aria-label={dismissLabel}>
      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round">
        <line x1="18" y1="6" x2="6" y2="18" />
        <line x1="6" y1="6" x2="18" y2="18" />
      </svg>
    </button>
  </div>
</div>

<style>
  /* Status is carried by the icon alone. A coloured edge or a tinted panel
     reads as a banner, and the toast sits over the account grid where it
     would compete with the cards for attention. */
  .toast {
    display: flex;
    overflow: hidden;
    max-width: min(380px, calc(100vw - 32px));
    background: var(--bg-overlay);
    background-image: var(--card-bg-image);
    border-radius: var(--radius-md);
    border: var(--border-width) var(--border-style) var(--border);
    box-shadow: var(--elevation-medium);
    margin-top: 8px;
    --toast-accent: var(--fg-subtle);
  }

  .toast.success {
    --toast-accent: var(--success);
  }

  .toast.error {
    --toast-accent: var(--danger);
  }

  .toast-body {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 9px 12px;
    min-width: 0;
  }

  .toast-icon {
    flex-shrink: 0;
    color: var(--toast-accent);
  }

  .toast-text {
    font-size: 12px;
    color: var(--fg);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .toast.error .toast-body {
    align-items: flex-start;
  }

  .toast.error .toast-icon {
    margin-top: 2px;
  }

  .toast.error .toast-text {
    white-space: normal;
    overflow-wrap: anywhere;
  }

  .toast-action {
    flex-shrink: 0;
    margin-left: 4px;
    padding: 3px 9px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg-muted);
    color: var(--fg);
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition:
      background 100ms,
      border-color 100ms;
  }

  .toast-action:hover {
    background: var(--bg-elevated);
    border-color: var(--bg-elevated);
  }

  .toast-close {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
    transition: background 100ms, color 100ms;
  }

  .toast-close:hover {
    background: color-mix(in srgb, var(--fg-muted) 18%, transparent);
    color: var(--fg);
  }
</style>
