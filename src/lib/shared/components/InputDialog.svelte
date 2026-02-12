<script lang="ts">
  import { onMount } from "svelte";

  let { title, placeholder = "", initialValue = "", onConfirm, onCancel }: {
    title: string;
    placeholder?: string;
    initialValue?: string;
    onConfirm: (value: string) => void;
    onCancel: () => void;
  } = $props();

  let value = $state(initialValue);
  let inputRef = $state<HTMLInputElement | null>(null);

  onMount(() => {
    inputRef?.focus();
    inputRef?.select();
  });

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onCancel();
    if (e.key === "Enter" && value.trim()) onConfirm(value.trim());
  }

  function handleSubmit() {
    if (value.trim()) onConfirm(value.trim());
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" onclick={onCancel}>
  <div class="dialog" onclick={(e) => e.stopPropagation()}>
    <span class="title">{title}</span>
    <input
      bind:this={inputRef}
      bind:value={value}
      class="input"
      {placeholder}
    />
    <div class="actions">
      <button class="btn-cancel" onclick={onCancel}>Cancel</button>
      <button class="btn-ok" onclick={handleSubmit} disabled={!value.trim()}>OK</button>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 90;
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
    width: 280px;
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

  .input {
    width: 100%;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg);
    color: var(--fg);
    font-size: 13px;
    outline: none;
    transition: border-color 120ms;
    box-sizing: border-box;
  }

  .input:focus {
    border-color: var(--bg-elevated);
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

  .btn-ok {
    padding: 6px 12px;
    border: none;
    border-radius: 4px;
    background: var(--fg);
    color: var(--bg);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: all 100ms;
  }

  .btn-ok:hover:not(:disabled) {
    background: #d4d4d8;
  }

  .btn-ok:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .btn-ok:active:not(:disabled) {
    transform: scale(0.97);
  }
</style>
