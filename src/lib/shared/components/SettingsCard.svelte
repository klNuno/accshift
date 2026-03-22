<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    accent,
    children,
  }: {
    title: string;
    accent?: string;
    children: Snippet;
  } = $props();
</script>

<section
  class="card"
  class:accented={!!accent}
  style={accent ? `--display-accent:${accent};` : undefined}
>
  <h3>{title}</h3>
  {@render children()}
</section>

<style>
  .card {
    background: color-mix(in srgb, var(--bg-card) 84%, #000 16%);
    border: 1px solid color-mix(in srgb, var(--border) 80%, #fff 20%);
    border-radius: 12px;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .card h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 700;
    color: var(--fg);
  }

  .card.accented {
    border-color: color-mix(in srgb, var(--display-accent) 32%, var(--border));
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--display-accent) 12%, transparent);
  }
</style>
