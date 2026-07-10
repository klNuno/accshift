<script lang="ts">
  import { onMount } from "svelte";
  import type { Snippet } from "svelte";

  let { title, width = "280px", onCancel, onKeydown, children, actions }: {
    title: string;
    width?: string;
    onCancel: () => void;
    onKeydown?: (e: KeyboardEvent) => void;
    children: Snippet;
    actions: Snippet;
  } = $props();

  let dialogEl = $state<HTMLDivElement | null>(null);
  const titleId = `dialog-title-${crypto.randomUUID().slice(0, 8)}`;

  onMount(() => {
    const trigger = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    return () => {
      if (trigger && document.contains(trigger)) trigger.focus();
    };
  });

  function getFocusable(): HTMLElement[] {
    if (!dialogEl) return [];
    return Array.from(
      dialogEl.querySelectorAll<HTMLElement>(
        'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'
      )
    ).filter((el) => el.offsetParent !== null);
  }

  function trapFocus(e: KeyboardEvent) {
    if (e.key !== "Tab") return;
    const focusable = getFocusable();
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    const withinDialog = active instanceof Node && dialogEl?.contains(active);
    if (e.shiftKey) {
      if (!withinDialog || active === first) {
        e.preventDefault();
        last.focus();
      }
    } else if (!withinDialog || active === last) {
      e.preventDefault();
      first.focus();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onCancel();
    trapFocus(e);
    onKeydown?.(e);
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" onclick={onCancel}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby={titleId} tabindex="-1" bind:this={dialogEl} style:width={width} onclick={(e) => e.stopPropagation()}>
    <span class="title" id={titleId}>{title}</span>
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
