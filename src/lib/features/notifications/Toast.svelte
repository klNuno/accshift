<script lang="ts">
  import { onMount } from "svelte";
  import type { ToastType, ToastAction } from "./store.svelte";

  let { message, durationMs = 3000, type = "info", toastAction = undefined, onDone }: {
    message: string;
    durationMs?: number | null;
    type?: ToastType;
    toastAction?: ToastAction;
    onDone: () => void;
  } = $props();

  onMount(() => {
    if (durationMs == null || !Number.isFinite(durationMs) || durationMs <= 0) {
      return;
    }
    const timer = setTimeout(() => {
      onDone();
    }, durationMs);
    return () => clearTimeout(timer);
  });

  function handleAction() {
    toastAction?.action();
    onDone();
  }
</script>

<div class="toast" class:success={type === "success"} class:error={type === "error"}>
  <div class="accent-bar"></div>
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
    <span class="toast-text">{message}</span>
    {#if toastAction}
      <button class="toast-action" onclick={handleAction}>{toastAction.label}</button>
    {/if}
  </div>
</div>

<style>
  .toast {
    display: flex;
    overflow: hidden;
    background: var(--bg-card);
    border-radius: 6px;
    border: 1px solid var(--border);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
    margin-top: 8px;
    --toast-accent: var(--fg-muted);
  }

  .toast.success {
    --toast-accent: #22c55e;
  }

  .toast.error {
    --toast-accent: #ef4444;
  }

  .accent-bar {
    width: 3px;
    flex-shrink: 0;
    background: var(--toast-accent);
  }

  .toast-body {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
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

  .toast-action {
    flex-shrink: 0;
    margin-left: 4px;
    padding: 2px 8px;
    border: none;
    border-radius: 3px;
    background: color-mix(in srgb, var(--toast-accent) 20%, transparent);
    color: var(--toast-accent);
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
    transition: background 100ms;
  }

  .toast-action:hover {
    background: color-mix(in srgb, var(--toast-accent) 32%, transparent);
  }
</style>
