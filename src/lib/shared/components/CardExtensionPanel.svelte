<script lang="ts">
  import type { CardExtensionContent, CardExtensionSection } from "$lib/shared/cardExtension";

  let {
    content,
    side = "right",
  }: {
    content: CardExtensionContent;
    side?: "left" | "right";
  } = $props();

  function sectionKey(section: CardExtensionSection, index: number) {
    return `${section.title ?? section.text ?? "section"}-${index}`;
  }
</script>

<div class="panel" class:left={side === "left"} class:right={side === "right"}>
  {#each content.sections as section, index (sectionKey(section, index))}
    <section class="section">
      {#if section.title}
        <div class="section-title">{section.title}</div>
      {/if}
      {#if section.text}
        <div class="section-text" class:loading-text={section.loading}>{section.text}</div>
      {/if}
      {#if section.lines?.length}
        <div class="section-lines">
          {#each section.lines as line (`${line}-${index}`)}
            <div class="section-line">{line}</div>
          {/each}
        </div>
      {/if}
      {#if section.chips?.length}
        <div class="chips">
          {#each section.chips as chip (`${chip.tone}-${chip.text}`)}
            <span class={`chip tone-${chip.tone}`}>{chip.text}</span>
          {/each}
        </div>
      {/if}
    </section>
  {/each}
</div>

<style>
  .panel {
    position: absolute;
    top: -6px;
    width: 176px;
    min-height: 100%;
    border-radius: 18px;
    border: 1px solid color-mix(in srgb, var(--border) 78%, #fff 22%);
    background:
      linear-gradient(180deg, color-mix(in srgb, var(--bg-card) 94%, #fff 6%), color-mix(in srgb, var(--bg-card) 88%, #000 12%));
    box-shadow:
      0 22px 44px rgba(0, 0, 0, 0.24),
      0 6px 14px rgba(0, 0, 0, 0.16);
    padding: 14px 12px 14px 52px;
    box-sizing: border-box;
    z-index: -1;
    display: flex;
    flex-direction: column;
    gap: 12px;
    pointer-events: none;
  }

  .panel.right {
    left: calc(100% - 18px);
  }

  .panel.left {
    right: calc(100% - 18px);
    padding: 14px 52px 14px 12px;
  }

  .panel::before {
    content: "";
    position: absolute;
    inset: 10px auto 10px 18px;
    width: 1px;
    background: color-mix(in srgb, var(--border) 82%, transparent);
    opacity: 0.8;
  }

  .panel.left::before {
    inset: 10px 18px 10px auto;
  }

  .section {
    display: flex;
    flex-direction: column;
    gap: 7px;
    min-width: 0;
    padding-bottom: 10px;
    border-bottom: 1px solid color-mix(in srgb, var(--border) 70%, transparent);
  }

  .section:last-child {
    padding-bottom: 0;
    border-bottom: none;
  }

  .section-title {
    font-size: 9px;
    font-weight: 700;
    line-height: 1.2;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--fg-subtle);
  }

  .section-text,
  .section-line {
    font-size: 11px;
    line-height: 1.45;
    color: var(--fg);
    word-break: break-word;
  }

  .section-text.loading-text {
    opacity: 0.9;
  }

  .section-lines {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-height: 20px;
    border-radius: 999px;
    padding: 0 8px;
    font-size: 9px;
    font-weight: 700;
    line-height: 1;
    letter-spacing: 0.02em;
  }

  .tone-red {
    background: rgba(239, 68, 68, 0.2);
    color: #fca5a5;
    border: 1px solid rgba(239, 68, 68, 0.5);
  }

  .tone-orange {
    background: rgba(251, 146, 60, 0.2);
    color: #fdba74;
    border: 1px solid rgba(251, 146, 60, 0.5);
  }

  .tone-blue {
    background: rgba(59, 130, 246, 0.18);
    color: #93c5fd;
    border: 1px solid rgba(59, 130, 246, 0.45);
  }

  .tone-green {
    background: rgba(16, 185, 129, 0.18);
    color: #86efac;
    border: 1px solid rgba(16, 185, 129, 0.45);
  }

  .tone-slate {
    background: rgba(148, 163, 184, 0.14);
    color: #cbd5e1;
    border: 1px solid rgba(148, 163, 184, 0.3);
  }

</style>
