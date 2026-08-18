<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import BaseDialog from "$lib/shared/components/BaseDialog.svelte";
  import type { MessageKey, TranslationParams } from "$lib/i18n";
  import { parseColor } from "./contrast";
  import {
    THEME_TOKEN_GROUPS,
    THEME_TOKEN_SPECS,
    TOKEN_KIND_EXAMPLE,
    getTokenSpec,
    type ThemeTokenGroup,
    type ThemeTokenKey,
    type ThemeTokenKind,
  } from "./tokens";
  import {
    MAX_THEME_CSS_LENGTH,
    resolveThemeTokens,
    serializeThemeDocument,
    validateThemeDocument,
    type ThemeDocument,
    type ThemeIssue,
  } from "./schema";
  import {
    ROOT_TOKENS,
    beginThemePreview,
    commitThemePreview,
    endThemePreview,
    getAllThemes,
    getThemeDocument,
    previewThemeDocument,
    resolveThemeSurfaceOpacities,
    saveThemeDocument,
    themeFromDocument,
  } from "./themes";

  let {
    source,
    backgroundOpacity,
    t,
    onCancel,
    onSaved,
  }: {
    source: ThemeDocument;
    backgroundOpacity: number;
    t: (key: MessageKey, params?: TranslationParams) => string;
    onCancel: () => void;
    onSaved: (document: ThemeDocument) => void;
  } = $props();

  const GROUP_LABELS: Record<ThemeTokenGroup, MessageKey> = {
    surface: "themeGroup.surface",
    text: "themeGroup.text",
    semantic: "themeGroup.semantic",
    shape: "themeGroup.shape",
    typography: "themeGroup.typography",
    motion: "themeGroup.motion",
  };

  const KIND_LABELS: Record<ThemeTokenKind, MessageKey> = {
    rgbTriplet: "themeKind.rgbTriplet",
    hexColor: "themeKind.hexColor",
    color: "themeKind.color",
    length: "themeKind.length",
    signedLength: "themeKind.signedLength",
    shadow: "themeKind.shadow",
    gradient: "themeKind.gradient",
    number: "themeKind.number",
    choice: "themeKind.choice",
    fontStack: "themeKind.fontStack",
  };

  /**
   * Choices that are words of ours get a translation. The ones that are CSS
   * keywords (`ridge`, `dotted`, `uppercase`) deliberately do not: the author
   * types those same words into the theme file, and a translated dropdown
   * would name them one thing here and another there.
   */
  const CHOICE_LABELS: Record<string, MessageKey> = {
    compact: "themeDensity.compact",
    cozy: "themeDensity.cozy",
    comfortable: "themeDensity.comfortable",
    circle: "themeAvatarShape.circle",
    rounded: "themeAvatarShape.rounded",
    square: "themeAvatarShape.square",
  };

  /** Same choice word, different meaning per token: `none` is a border style
   *  under borderStyle and a smoothing mode under fontSmoothing. */
  const CHOICE_LABELS_BY_TOKEN: Partial<Record<ThemeTokenKey, Record<string, MessageKey>>> = {
    fontSmoothing: { auto: "themeSmoothing.auto", none: "themeSmoothing.none" },
  };

  function choiceLabel(key: ThemeTokenKey, choice: string): string {
    const messageKey = CHOICE_LABELS_BY_TOKEN[key]?.[choice] ?? CHOICE_LABELS[choice];
    return messageKey ? t(messageKey) : choice;
  }

  // The editor opens on a document and owns it from there: the parent mounts a
  // fresh one per session, so the fields start from this snapshot and the prop
  // is never read again.
  const initial = untrack(() => source);
  let name = $state(initial.name);
  let author = $state(initial.author ?? "");
  let version = $state(initial.version ?? "");
  let colorScheme = $state<"dark" | "light">(initial.colorScheme);
  let base = $state(initial.extends ?? "");
  let glass = $state(Boolean(initial.glass));
  let tokens = $state<Partial<Record<ThemeTokenKey, string>>>({ ...initial.tokens });
  let css = $state(initial.css ?? "");
  let saved = false;

  // Snapshot the running theme now, at component init: the preview effect below
  // fires on mount and would otherwise snapshot itself, leaving cancel with
  // nothing to put back.
  beginThemePreview();
  onDestroy(() => {
    if (!saved) endThemePreview();
  });

  const draft = $derived<ThemeDocument>({
    schemaVersion: initial.schemaVersion,
    id: initial.id,
    name: name.trim() || initial.id,
    ...(author.trim() ? { author: author.trim() } : {}),
    ...(version.trim() ? { version: version.trim() } : {}),
    colorScheme,
    ...(base ? { extends: base } : {}),
    ...(glass ? { glass: true } : {}),
    tokens,
    ...(css.trim() ? { css } : {}),
  });

  const resolved = $derived(resolveThemeTokens(draft, getThemeDocument, ROOT_TOKENS));
  // Card surfaces are painted at this alpha, so this is what the contrast check
  // has to measure text against on a glass theme.
  const cardAlpha = $derived(
    resolveThemeSurfaceOpacities(themeFromDocument(draft), backgroundOpacity).cardOpacity,
  );
  const issues = $derived(validateThemeDocument(draft, resolved, { cardAlpha }));
  // Problems that cannot be saved: each one would be dropped on the next load,
  // leaving a theme that silently differs from what the editor showed. A tight
  // contrast is a judgement call and stays the author's to make.
  const BLOCKING_CODES = ["invalidValue", "unsafeCss", "cssTooLong"];
  const blocked = $derived(issues.some((issue) => BLOCKING_CODES.includes(issue.code)));

  $effect(() => {
    previewThemeDocument(draft);
  });

  /** Bases that would loop back into this theme are not offered. */
  function reachesSelf(document: ThemeDocument | undefined, depth = 0): boolean {
    if (!document || depth > 8) return false;
    if (document.id === initial.id) return true;
    return document.extends ? reachesSelf(getThemeDocument(document.extends), depth + 1) : false;
  }

  const baseOptions = getAllThemes()
    .filter((theme) => theme.id !== initial.id && !reachesSelf(theme.document))
    .map((theme) => ({ id: theme.id, label: theme.displayName ?? theme.id }));

  function tokenLabel(key: string): string {
    const spec = getTokenSpec(key);
    return spec ? t(spec.labelKey) : key;
  }

  function issueText(issue: ThemeIssue): string {
    switch (issue.code) {
      case "invalidValue": {
        const kind = issue.expected as ThemeTokenKind;
        return t("themeEditor.issueInvalidValue", {
          token: tokenLabel(String(issue.token)),
          kind: t(KIND_LABELS[kind]),
          example: TOKEN_KIND_EXAMPLE[kind],
        });
      }
      case "unknownToken":
        return t("themeEditor.issueUnknownToken", { token: String(issue.token) });
      case "missingToken":
        return t("themeEditor.issueMissingToken", { token: tokenLabel(String(issue.token)) });
      case "unknownBase":
        return t("themeEditor.issueUnknownBase", { base: String(issue.base) });
      case "unsafeCss":
        return t("themeEditor.issueUnsafeCss", { construct: String(issue.construct) });
      case "cssTooLong":
        return t("themeEditor.issueCssTooLong", {
          length: Number(issue.length),
          limit: Number(issue.limit),
        });
      case "contrast":
        return t("themeEditor.issueContrast", {
          token: tokenLabel(String(issue.token)),
          against: tokenLabel(String(issue.against)),
          ratio: issue.ratio,
          target: issue.target,
        });
    }
  }

  const issueByToken = $derived(
    new Map(issues.filter((issue) => issue.token).map((issue) => [String(issue.token), issue])),
  );

  function setToken(key: ThemeTokenKey, value: string) {
    tokens = { ...tokens, [key]: value };
  }

  function resetToken(key: ThemeTokenKey) {
    const next = { ...tokens };
    delete next[key];
    tokens = next;
  }

  /** Name of the theme a token is currently coming from, for the badge title. */
  function inheritedFrom(key: ThemeTokenKey): string {
    for (const id of resolved.chain.slice(1)) {
      const document = getThemeDocument(id);
      if (document?.tokens[key] !== undefined) return document.name;
    }
    return t("themeEditor.builtInBase");
  }

  function toHex(value: string): string {
    const rgb = parseColor(value);
    if (!rgb) return "#000000";
    const channel = (n: number) =>
      Math.round(Math.min(255, Math.max(0, n)))
        .toString(16)
        .padStart(2, "0");
    return `#${channel(rgb.r)}${channel(rgb.g)}${channel(rgb.b)}`;
  }

  function fromHex(hex: string, kind: ThemeTokenKind): string {
    if (kind !== "rgbTriplet") return hex;
    const rgb = parseColor(hex);
    return rgb ? `${rgb.r} ${rgb.g} ${rgb.b}` : hex;
  }

  const COLOR_KINDS: readonly ThemeTokenKind[] = ["rgbTriplet", "hexColor", "color"];

  /** Code, so it is not translated: a CSS selector reads the same everywhere. */
  const CSS_PLACEHOLDER = ".account-card { letter-spacing: 0.02em; }";

  async function save() {
    if (blocked) return;
    const document = draft;
    await saveThemeDocument(document);
    saved = true;
    commitThemePreview(document);
    onSaved(document);
  }

  function copyToClipboard() {
    void navigator.clipboard.writeText(serializeThemeDocument(draft));
  }
</script>

<BaseDialog title={t("themeEditor.title")} width="min(720px, 92vw)" {onCancel}>
  <div class="editor-body">
    <p class="beta-note">
      <span class="beta-pill">{t("themeEditor.beta")}</span>
      {t("themeEditor.betaNote")}
    </p>

    <div class="meta-grid">
      <label class="field">
        <span class="field-label">{t("themeEditor.name")}</span>
        <input class="text-input" type="text" maxlength="64" bind:value={name} />
      </label>
      <label class="field">
        <span class="field-label">{t("themeEditor.author")}</span>
        <input class="text-input" type="text" maxlength="64" bind:value={author} />
      </label>
      <label class="field">
        <span class="field-label">{t("themeEditor.version")}</span>
        <input class="text-input" type="text" maxlength="64" bind:value={version} />
      </label>
      <label class="field">
        <span class="field-label">{t("themeEditor.base")}</span>
        <select class="text-input select-input" bind:value={base}>
          <option value="">{t("common.none")}</option>
          {#each baseOptions as option (option.id)}
            <option value={option.id}>{option.label}</option>
          {/each}
        </select>
      </label>
      <label class="field">
        <span class="field-label">{t("themeEditor.colorScheme")}</span>
        <select class="text-input select-input" bind:value={colorScheme}>
          <option value="dark">{t("theme.dark")}</option>
          <option value="light">{t("theme.light")}</option>
        </select>
      </label>
      <label class="field checkbox-field">
        <input type="checkbox" bind:checked={glass} />
        <span class="field-label">{t("themeEditor.glass")}</span>
      </label>
    </div>

    {#each THEME_TOKEN_GROUPS as group (group)}
      <section class="token-group">
        <h4>{t(GROUP_LABELS[group])}</h4>
        {#each THEME_TOKEN_SPECS.filter((spec) => spec.group === group) as spec (spec.key)}
          {@const overridden = tokens[spec.key] !== undefined}
          {@const value = tokens[spec.key] ?? resolved.tokens[spec.key]}
          {@const issue = issueByToken.get(spec.key)}
          <div class="token-row" class:overridden>
            <span class="token-label">
              {t(spec.labelKey)}
              {#if !overridden}
                <span class="inherited" title={inheritedFrom(spec.key)}
                  >{t("themeEditor.inherited")}</span
                >
              {/if}
            </span>
            <span class="token-control">
              {#if COLOR_KINDS.includes(spec.kind)}
                <input
                  class="color-input"
                  type="color"
                  value={toHex(value)}
                  aria-label={t(spec.labelKey)}
                  oninput={(e) =>
                    setToken(spec.key, fromHex(e.currentTarget.value, spec.kind))}
                />
                <input
                  class="text-input"
                  type="text"
                  {value}
                  aria-label={t(spec.labelKey)}
                  oninput={(e) => setToken(spec.key, e.currentTarget.value)}
                />
              {:else if spec.kind === "choice"}
                <select
                  class="text-input select-input"
                  {value}
                  aria-label={t(spec.labelKey)}
                  onchange={(e) => setToken(spec.key, e.currentTarget.value)}
                >
                  {#each spec.choices ?? [] as choice (choice)}
                    <option value={choice}>{choiceLabel(spec.key, choice)}</option>
                  {/each}
                </select>
              {:else}
                <input
                  class="text-input"
                  type="text"
                  {value}
                  placeholder={TOKEN_KIND_EXAMPLE[spec.kind]}
                  aria-label={t(spec.labelKey)}
                  oninput={(e) => setToken(spec.key, e.currentTarget.value)}
                />
              {/if}
              <button
                type="button"
                class="reset-btn"
                disabled={!overridden}
                title={t("themeEditor.reset")}
                aria-label={t("themeEditor.reset")}
                onclick={() => resetToken(spec.key)}>&#8634;</button
              >
            </span>
            {#if issue}
              <span class="token-issue" class:error={issue.level === "error"}
                >{issueText(issue)}</span
              >
            {/if}
          </div>
        {/each}
      </section>
    {/each}

    <section class="token-group">
      <h4>{t("themeEditor.customCss")}</h4>
      <p class="css-help">{t("themeEditor.customCssHelp")}</p>
      <textarea
        class="css-input"
        rows="6"
        spellcheck="false"
        maxlength={MAX_THEME_CSS_LENGTH}
        placeholder={CSS_PLACEHOLDER}
        aria-label={t("themeEditor.customCss")}
        bind:value={css}
      ></textarea>
    </section>

    <section class="token-group">
      <h4>{t("themeEditor.issues")}</h4>
      {#if issues.length === 0}
        <p class="issue-line ok">{t("themeEditor.noIssues")}</p>
      {:else}
        {#each issues as issue, index (index)}
          <p class="issue-line" class:error={issue.level === "error"}>{issueText(issue)}</p>
        {/each}
      {/if}
    </section>
  </div>

  {#snippet actions()}
    <button type="button" class="dialog-btn" onclick={copyToClipboard}
      >{t("themeEditor.export")}</button
    >
    <button type="button" class="dialog-btn" onclick={onCancel}>{t("common.cancel")}</button>
    <button
      type="button"
      class="dialog-btn primary"
      disabled={blocked}
      title={blocked ? t("themeEditor.fixErrorsFirst") : ""}
      onclick={save}>{t("themeEditor.save")}</button
    >
  {/snippet}
</BaseDialog>

<style>
  .editor-body {
    display: flex;
    flex-direction: column;
    gap: 14px;
    max-height: min(62vh, 560px);
    overflow-y: auto;
    padding-right: 4px;
  }

  .beta-note {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    font-size: 11px;
    color: var(--fg-subtle);
  }

  .beta-pill {
    flex-shrink: 0;
    padding: 1px 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 9px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--fg-muted);
  }

  .css-help {
    margin: 0 0 6px;
    font-size: 11px;
    color: var(--fg-subtle);
  }

  .css-input {
    width: 100%;
    resize: vertical;
    padding: 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    color: var(--fg);
    /* Monospace here is the exception the rule allows: the field holds CSS. */
    font-family: ui-monospace, "Cascadia Mono", Menlo, Consolas, monospace;
    font-size: 11px;
    line-height: 1.5;
  }

  .meta-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 10px;
  }

  .checkbox-field {
    flex-direction: row;
    align-items: center;
    gap: 8px;
    align-self: end;
  }

  .token-group h4 {
    margin: 0 0 6px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--fg-subtle);
  }

  .token-row {
    display: grid;
    grid-template-columns: minmax(140px, 40%) 1fr;
    align-items: center;
    gap: 8px;
    padding: 3px 0;
  }

  .token-label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--fg-muted);
  }

  .token-row.overridden .token-label {
    color: var(--fg);
  }

  .inherited {
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 1px 4px;
    border-radius: var(--radius-sm);
    background: var(--bg-muted);
    color: var(--fg-subtle);
  }

  .token-control {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .token-control .text-input {
    flex: 1;
    min-width: 0;
  }

  .color-input {
    width: 28px;
    height: 24px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: transparent;
    cursor: pointer;
  }

  .reset-btn {
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--fg-subtle);
    font-size: 13px;
    line-height: 1;
    padding: 4px 6px;
    cursor: pointer;
  }

  .reset-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .token-issue,
  .issue-line {
    grid-column: 1 / -1;
    margin: 0;
    font-size: 11px;
    color: var(--warning);
  }

  .token-issue.error,
  .issue-line.error {
    color: var(--danger);
  }

  .issue-line.ok {
    color: var(--fg-subtle);
  }

  .dialog-btn {
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg-card);
    color: var(--fg-muted);
    font-size: 12px;
    padding: 5px 12px;
    cursor: pointer;
  }

  .dialog-btn:hover {
    background: var(--bg-card-hover);
    color: var(--fg);
  }

  .dialog-btn.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-fg);
  }

  .dialog-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
