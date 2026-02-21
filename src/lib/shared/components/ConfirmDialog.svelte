<script lang="ts">
  import { onMount } from "svelte";
  import { DEFAULT_LOCALE, translate, type Locale } from "$lib/i18n";

  let { title, message, confirmLabel = "", cancelLabel = "", onConfirm, onCancel, locale = DEFAULT_LOCALE }: {
    title: string;
    message: string;
    confirmLabel?: string;
    cancelLabel?: string;
    onConfirm: () => void;
    onCancel: () => void;
    locale?: Locale;
  } = $props();

  let confirmRef = $state<HTMLButtonElement | null>(null);

  onMount(() => {
    confirmRef?.focus();
  });

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onCancel();
    if (e.key === "Enter") onConfirm();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" onclick={onCancel}>
  <div class="dialog" onclick={(e) => e.stopPropagation()}>
    <span class="title">{title}</span>
    <p class="message">{message}</p>
    <div class="actions">
      <button class="btn-cancel" onclick={onCancel}>{cancelLabel || translate(locale, "common.cancel")}</button>
      <button bind:this={confirmRef} class="btn-confirm" onclick={onConfirm}>{confirmLabel || translate(locale, "common.confirm")}</button>
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
    animation: fadeIn 100ms ease-out;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .dialog {
    width: min(360px, calc(100vw - 24px));
    padding: 16px;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.4);
    display: flex;
    flex-direction: column;
    gap: 12px;
    animation: slideIn 120ms ease-out;
  }

  @keyframes slideIn {
    from { opacity: 0; transform: scale(0.96) translateY(8px); }
    to { opacity: 1; transform: scale(1) translateY(0); }
  }

  .title {
    font-size: 13px;
    font-weight: 600;
    color: var(--fg);
  }

  .message {
    margin: 0;
    font-size: 12px;
    line-height: 1.35;
    color: var(--fg-muted);
    white-space: pre-line;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .btn-cancel {
    padding: 6px 12px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: transparent;
    color: var(--fg-muted);
    font-size: 12px;
    cursor: pointer;
    transition: all 100ms;
  }

  .btn-cancel:hover {
    background: var(--bg-muted);
    color: var(--fg);
  }

  .btn-confirm {
    padding: 6px 12px;
    border: none;
    border-radius: 4px;
    background: #dc2626;
    color: #fff;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: all 100ms;
  }

  .btn-confirm:hover {
    background: #b91c1c;
  }

  .btn-confirm:active {
    transform: scale(0.97);
  }
</style>
