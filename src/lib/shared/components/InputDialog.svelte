<script lang="ts">
  import { onMount } from "svelte";
  import { DEFAULT_LOCALE, translate, type Locale } from "$lib/i18n";
  import { trackDependencies } from "$lib/shared/trackDependencies";
  import BaseDialog from "./BaseDialog.svelte";

  let { title, placeholder = "", initialValue = "", allowEmpty = false, onConfirm, onCancel, locale = DEFAULT_LOCALE }: {
    title: string;
    placeholder?: string;
    initialValue?: string;
    allowEmpty?: boolean;
    onConfirm: (value: string) => void;
    onCancel: () => void;
    locale?: Locale;
  } = $props();

  let value = $state("");
  let inputRef = $state<HTMLInputElement | null>(null);

  $effect(() => {
    trackDependencies(initialValue);
    value = initialValue;
  });

  onMount(() => {
    inputRef?.focus();
    inputRef?.select();
  });

  function submit() {
    if (allowEmpty || value.trim()) onConfirm(allowEmpty ? value : value.trim());
  }
</script>

<BaseDialog
  {title}
  {onCancel}
  onKeydown={(e) => { if (e.key === "Enter") submit(); }}
>
  <input
    bind:this={inputRef}
    bind:value={value}
    class="input"
    {placeholder}
  />

  {#snippet actions()}
    <button class="btn-cancel" onclick={onCancel}>{translate(locale, "common.cancel")}</button>
    <button class="btn-ok" onclick={submit} disabled={!allowEmpty && !value.trim()}>{translate(locale, "common.ok")}</button>
  {/snippet}
</BaseDialog>

<style>
  .input {
    width: 100%;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg-solid);
    color: var(--fg);
    font-size: 13px;
    outline: none;
    transition: border-color 120ms;
    box-sizing: border-box;
  }

  .input:focus {
    border-color: var(--bg-elevated);
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
    color: var(--bg-solid);
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
