<script lang="ts">
  import { onMount } from "svelte";

  let { message, onDone }: {
    message: string;
    onDone: () => void;
  } = $props();

  onMount(() => {
    const timer = setTimeout(() => {
      // Trigger removal which will be handled by the parent list's transition
      onDone();
    }, 3000);
    return () => clearTimeout(timer);
  });
</script>

<div class="toast">
  {message}
</div>

<style>
  .toast {
    padding: 8px 16px;
    background: var(--bg-muted);
    color: var(--fg);
    font-size: 12px;
    border-radius: 6px;
    border: 1px solid var(--bg-elevated);
    /* Animation handled by svelte transition in parent */
    pointer-events: none;
    margin-top: 8px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
  }
</style>
