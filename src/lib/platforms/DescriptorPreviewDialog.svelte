<!--
  What a descriptor file would add, before it adds it.

  The plan comes from the same code path the dry run uses, so this dialog shows
  the real switch: the folders the descriptor may touch, every file, registry
  value and process it would read, copy, write or close, and the warnings a real
  switch would hit. Nothing has been written by the time this is on screen.
-->
<script lang="ts">
  import BaseDialog from "$lib/shared/components/BaseDialog.svelte";
  import type { DescriptorPreview, PlanStep } from "./descriptors";
  import type { MessageKey, TranslationParams } from "$lib/i18n";

  let {
    preview,
    busy = false,
    t,
    onCancel,
    onConfirm,
  }: {
    preview: DescriptorPreview;
    busy?: boolean;
    t: (key: MessageKey, params?: TranslationParams) => string;
    onCancel: () => void;
    onConfirm: () => void;
  } = $props();

  const ACTION_KEYS: Record<PlanStep["action"], MessageKey> = {
    read: "descriptor.actionRead",
    capture: "descriptor.actionCapture",
    restore: "descriptor.actionRestore",
    delete: "descriptor.actionDelete",
    close: "descriptor.actionClose",
    launch: "descriptor.actionLaunch",
  };

  let systems = $derived(Object.keys(preview.descriptor.os).join(", "));
  let canAdd = $derived(!preview.blocked && !busy);
</script>

<BaseDialog
  title={t("descriptor.previewTitle")}
  width="min(560px, calc(100vw - 24px))"
  {onCancel}
>
  <div class="body">
    <div class="head">
      <span class="name">{preview.descriptor.name}</span>
      <code>{preview.descriptor.id}</code>
      <span class="from">{preview.source}</span>
    </div>
    <p class="line">{t("descriptor.previewOs", { list: systems })}</p>

    {#if preview.blocked}
      <p class="blocked">{preview.blocked}</p>
    {:else if preview.replaces}
      <p class="line warn">{t("descriptor.previewReplaces")}</p>
    {/if}

    {#if preview.plan}
      {@const plan = preview.plan}
      <section>
        <h4>{t("descriptor.previewRoots")}</h4>
        <ul class="paths">
          {#each plan.roots as root (root)}
            <li>{root}</li>
          {/each}
        </ul>
      </section>

      <section>
        <h4>{t("descriptor.previewSteps")}</h4>
        <ul class="steps">
          {#each plan.steps as step, index (`${step.action}:${step.target}:${index}`)}
            <li>
              <span class="action">{t(ACTION_KEYS[step.action])}</span>
              <span class="target">{step.target}</span>
              {#if step.snapshot}
                <span class="snapshot">{step.snapshot}</span>
              {/if}
              {#if step.note}
                <span class="note">{step.note}</span>
              {/if}
            </li>
          {/each}
        </ul>
      </section>

      {#if plan.warnings.length}
        <section>
          <h4>{t("descriptor.previewWarnings")}</h4>
          <ul class="warnings">
            {#each plan.warnings as warning (warning)}
              <li>{warning}</li>
            {/each}
          </ul>
        </section>
      {/if}
    {:else if preview.planProblem}
      <p class="line warn">
        {t("descriptor.previewNoPlan", { problem: preview.planProblem })}
      </p>
    {/if}

    <p class="line quiet">{t("descriptor.previewNothingWritten")}</p>
  </div>

  {#snippet actions()}
    <button class="btn-cancel" onclick={onCancel}>{t("common.cancel")}</button>
    <button class="btn-confirm" disabled={!canAdd} onclick={onConfirm}>
      {preview.replaces ? t("descriptor.replace") : t("common.add")}
    </button>
  {/snippet}
</BaseDialog>

<style>
  /* The plan is the point of this dialog and it can be long, so the list
     scrolls inside the dialog rather than pushing the buttons off screen. */
  .body {
    display: flex;
    flex-direction: column;
    gap: 12px;
    max-height: min(60vh, 460px);
    overflow-y: auto;
    padding-right: 4px;
  }

  .head {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 8px;
  }

  .name {
    font-size: 13px;
    font-weight: 600;
    color: var(--fg);
  }

  .from {
    margin-left: auto;
    font-size: 11px;
    color: var(--fg-subtle);
  }

  .line {
    margin: 0;
    font-size: 12px;
    color: var(--fg-muted);
  }

  .quiet {
    color: var(--fg-subtle);
  }

  .warn {
    color: var(--fg);
  }

  .blocked {
    margin: 0;
    padding: 8px 10px;
    border-radius: 6px;
    background: color-mix(in srgb, var(--danger) 14%, transparent);
    color: var(--fg);
    font-size: 12px;
  }

  section {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  h4 {
    margin: 0;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--fg-subtle);
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  li {
    font-size: 11px;
    color: var(--fg-muted);
    overflow-wrap: anywhere;
  }

  .paths li,
  .steps .target,
  .steps .snapshot {
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
  }

  .action {
    display: inline-block;
    min-width: 58px;
    color: var(--fg);
  }

  .snapshot::before {
    content: "< ";
    color: var(--fg-subtle);
  }

  .note,
  .warnings li {
    color: var(--fg-subtle);
  }

  .note::before {
    content: " ";
  }

  .btn-cancel {
    padding: 6px 12px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: transparent;
    color: var(--fg-muted);
    font-size: 12px;
    cursor: pointer;
  }

  .btn-cancel:hover {
    background: var(--bg-muted);
    color: var(--fg);
  }

  /* The app's primary button: light on dark, no colour of its own. Adding a
     platform is not destructive, so it must not read as the danger red the
     confirm dialogs use. */
  .btn-confirm {
    padding: 6px 12px;
    border: none;
    border-radius: 4px;
    background: var(--fg);
    color: var(--bg-solid);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }

  .btn-confirm:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-confirm:not(:disabled):hover {
    filter: brightness(0.9);
  }
</style>
