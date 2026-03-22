<script lang="ts">
  import type { Snippet } from "svelte";

  let { title, width = "280px", onCancel, onKeydown, children, actions }: {
    title: string;
    width?: string;
    onCancel: () => void;
    onKeydown?: (e: KeyboardEvent) => void;
    children: Snippet;
    actions: Snippet;
  } = $props();

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onCancel();
    onKeydown?.(e);
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" onclick={onCancel}>
  <div class="dialog" style:width={width} onclick={(e) => e.stopPropagation()}>
    <span class="title">{title}</span>
    {@render children()}
    <div class="actions">
      {@render actions()}
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 1100;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(4px);
    animation: overlay-fade-in 100ms ease-out;
  }

  .dialog {
    padding: 16px;
    background: var(--bg-overlay);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.4);
    display: flex;
    flex-direction: column;
    gap: 12px;
    animation: dialog-slide-in 120ms ease-out;
  }

  .title {
    font-size: 13px;
    font-weight: 600;
    color: var(--fg);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
