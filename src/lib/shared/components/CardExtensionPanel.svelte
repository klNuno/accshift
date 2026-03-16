<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { CardExtensionContent, CardExtensionSection } from "$lib/shared/cardExtension";

  let { content }: { content: CardExtensionContent } = $props();

  function sectionKey(section: CardExtensionSection, index: number) {
    return `${section.title ?? section.text ?? "section"}-${index}`;
  }

  function openLink(url: string) {
    void invoke("open_url", { url });
  }
</script>

<div class="details">
  {#each content.sections as section, index (sectionKey(section, index))}
    <section class="section" class:loading={section.loading}>
      {#if section.title}
        <div class="section-title">{section.title}</div>
      {/if}
      {#if section.text}
        <div class="section-text-row">
          {#if section.loading}
            <span class="status-dot" aria-hidden="true"></span>
          {/if}
          <div class="section-text">{section.text}</div>
        </div>
      {/if}
      {#if section.link}
        <button class="section-link" onclick={() => openLink(section.link!.url)}>
          {section.link.label}
        </button>
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
  .details {
    width: 100%;
    max-width: 100%;
    min-width: 0;
    height: 100%;
    padding: 10px 4px 10px 10px;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 10px;
    pointer-events: none;
  }

  .section {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
    padding-bottom: 8px;
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

  .section-text-row {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    min-width: 0;
  }

  .status-dot {
    width: 6px;
    height: 6px;
    margin-top: 5px;
    border-radius: 999px;
    flex: 0 0 auto;
    background: color-mix(in srgb, var(--card-custom-color, #60a5fa) 52%, #ffffff);
    opacity: 0.9;
    animation: pulseDot 1.5s ease-in-out infinite;
  }

  @keyframes pulseDot {
    0%, 100% {
      transform: scale(0.9);
      opacity: 0.72;
    }
    50% {
      transform: scale(1.05);
      opacity: 1;
    }
  }

  .section-text,
  .section-line {
    font-size: 11px;
    line-height: 1.45;
    color: var(--fg);
    word-break: break-word;
    overflow-wrap: anywhere;
    min-width: 0;
  }

  .section.loading .section-text {
    color: color-mix(in srgb, var(--fg) 88%, var(--fg-muted) 12%);
  }

  .section-link {
    all: unset;
    font-size: 10px;
    line-height: 1.3;
    color: #93c5fd;
    text-decoration: underline;
    cursor: pointer;
    pointer-events: auto;
    word-break: break-word;
    overflow-wrap: anywhere;
  }

  .section-link:hover {
    color: #bfdbfe;
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
