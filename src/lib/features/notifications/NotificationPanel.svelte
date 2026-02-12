<script lang="ts">
  import { getNotifications, clearNotifications } from "./store";
  import type { AppNotification } from "./types";

  let { onClose }: { onClose: () => void } = $props();

  let notifications = $state<AppNotification[]>(getNotifications());

  function clearAll() {
    clearNotifications();
    notifications = [];
  }

  function formatTime(timestamp: number): string {
    const diff = Date.now() - timestamp;
    const mins = Math.floor(diff / 60000);
    if (mins < 1) return "just now";
    if (mins < 60) return `${mins}m ago`;
    const hours = Math.floor(mins / 60);
    if (hours < 24) return `${hours}h ago`;
    const days = Math.floor(hours / 24);
    return `${days}d ago`;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" onclick={onClose}>
  <div class="panel" onclick={(e) => e.stopPropagation()}>
    <div class="header">
      <span class="title">Notifications</span>
      <div class="header-actions">
        {#if notifications.length > 0}
          <button class="clear-btn" onclick={clearAll}>Clear all</button>
        {/if}
        <button class="close-btn" onclick={onClose}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>
      </div>
    </div>

    <div class="body">
      {#if notifications.length === 0}
        <div class="empty">No notifications</div>
      {:else}
        {#each notifications as notif (notif.id)}
          <div class="notif-item">
            <p class="notif-message">{notif.message}</p>
            <span class="notif-time">{formatTime(notif.timestamp)}</span>
          </div>
        {/each}
      {/if}
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 80;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(4px);
    animation: fadeIn 120ms ease-out;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .panel {
    width: 340px;
    max-height: 400px;
    display: flex;
    flex-direction: column;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.4);
    animation: slideIn 150ms ease-out;
  }

  @keyframes slideIn {
    from { opacity: 0; transform: scale(0.96) translateY(8px); }
    to { opacity: 1; transform: scale(1) translateY(0); }
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .title {
    font-size: 13px;
    font-weight: 600;
    color: var(--fg);
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .clear-btn {
    padding: 4px 8px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--fg-muted);
    font-size: 11px;
    cursor: pointer;
    transition: all 100ms;
  }

  .clear-btn:hover {
    background: var(--bg-muted);
    color: var(--fg);
  }

  .close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
    transition: all 100ms;
  }

  .close-btn:hover {
    background: var(--bg-muted);
    color: var(--fg);
  }

  .body {
    overflow-y: auto;
    padding: 8px;
  }

  .empty {
    padding: 24px;
    text-align: center;
    font-size: 12px;
    color: var(--fg-subtle);
  }

  .notif-item {
    padding: 10px 12px;
    border-radius: 6px;
    transition: background 80ms;
  }

  .notif-item:hover {
    background: var(--bg-muted);
  }

  .notif-message {
    font-size: 12px;
    color: var(--fg);
    margin: 0 0 4px;
    line-height: 1.4;
  }

  .notif-time {
    font-size: 10px;
    color: var(--fg-subtle);
  }
</style>
