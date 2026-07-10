<script lang="ts">
  import { onMount } from "svelte";
  import { DEFAULT_LOCALE, translate, type Locale } from "$lib/i18n";
  import BaseDialog from "./BaseDialog.svelte";

  let { title, message, confirmLabel = "", cancelLabel = "", confirmColor = "", onConfirm, onCancel, locale = DEFAULT_LOCALE }: {
    title: string;
    message: string;
    confirmLabel?: string;
    cancelLabel?: string;
    confirmColor?: string;
    onConfirm: () => void;
    onCancel: () => void;
    locale?: Locale;
  } = $props();

  let confirmRef = $state<HTMLButtonElement | null>(null);
  let cancelRef = $state<HTMLButtonElement | null>(null);

  // Default styling is the danger red, so no explicit color means destructive.
  let destructive = $derived(!confirmColor || confirmColor.includes("danger"));

  onMount(() => {
    (destructive ? cancelRef : confirmRef)?.focus();
  });

  function handleEnter(e: KeyboardEvent) {
    if (e.key !== "Enter") return;
    const active = document.activeElement;
    // A focused button handles Enter natively; don't force confirm over it.
    if (active === confirmRef || active === cancelRef) return;
    onConfirm();
  }
</script>

<BaseDialog
  {title}
  width="min(360px, calc(100vw - 24px))"
  {onCancel}
  onKeydown={handleEnter}
>
  <p class="message">{message}</p>

  {#snippet actions()}
    <button bind:this={cancelRef} class="btn-cancel" onclick={onCancel}>{cancelLabel || translate(locale, "common.cancel")}</button>
    <button bind:this={confirmRef} class="btn-confirm" style={confirmColor ? `--confirm-bg: ${confirmColor};` : ""} onclick={onConfirm}>{confirmLabel || translate(locale, "common.confirm")}</button>
  {/snippet}
</BaseDialog>

<style>
  .message {
    margin: 0;
    font-size: 12px;
    line-height: 1.35;
    color: var(--fg-muted);
    white-space: pre-line;
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
    background: var(--confirm-bg, var(--danger));
    color: #fff;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: all 100ms;
  }

  .btn-confirm:hover {
    filter: brightness(0.85);
  }

  .btn-confirm:active {
    transform: scale(0.97);
  }
</style>
