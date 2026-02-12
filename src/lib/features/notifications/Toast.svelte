<script lang="ts">
  import { onMount } from "svelte";

  let { message, onDone }: {
    message: string;
    onDone: () => void;
  } = $props();

  onMount(() => {
    const timer = setTimeout(onDone, 2000);
    return () => clearTimeout(timer);
  });
</script>

<div class="toast">
  {message}
</div>

<style>
  .toast {
    position: fixed;
    bottom: 16px;
    left: 50%;
    transform: translateX(-50%);
    padding: 8px 16px;
    background: var(--bg-muted);
    color: var(--fg);
    font-size: 12px;
    border-radius: 6px;
    border: 1px solid var(--bg-elevated);
    z-index: 200;
    animation: toastIn 150ms ease-out;
    pointer-events: none;
  }

  @keyframes toastIn {
    from { opacity: 0; transform: translateX(-50%) translateY(8px); }
    to { opacity: 1; transform: translateX(-50%) translateY(0); }
  }
</style>
