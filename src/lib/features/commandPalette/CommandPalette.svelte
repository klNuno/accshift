<script lang="ts">
  import type { MessageKey, TranslationParams } from "$lib/i18n";
  import type { PaletteCommand, PaletteSection } from "./registry";
  import { fuzzyScoreAll } from "./fuzzy";

  type Props = {
    commands: PaletteCommand[];
    onClose: () => void;
    t: (key: MessageKey, params?: TranslationParams) => string;
  };

  let { commands, onClose, t }: Props = $props();

  let query = $state("");
  let activeIndex = $state(0);
  let inputRef = $state<HTMLInputElement | null>(null);
  let listRef = $state<HTMLDivElement | null>(null);

  const SECTION_ORDER: PaletteSection[] = ["accounts", "actions", "navigation"];
  const SECTION_LABEL_KEYS: Record<PaletteSection, MessageKey> = {
    accounts: "palette.sectionAccounts",
    actions: "palette.sectionActions",
    navigation: "palette.sectionNavigation",
  };
  const EMPTY_QUERY_ACCOUNT_LIMIT = 6;

  let filtered = $derived.by(() => {
    const q = query.trim();
    if (!q) {
      // Idle view: a few accounts on top, then every action and destination.
      const accounts = commands
        .filter((c) => c.section === "accounts")
        .slice(0, EMPTY_QUERY_ACCOUNT_LIMIT);
      const rest = commands.filter((c) => c.section !== "accounts");
      return [...accounts, ...rest];
    }
    const scored: Array<{ command: PaletteCommand; score: number }> = [];
    for (const command of commands) {
      const score = fuzzyScoreAll(q, command.title, command.keywords);
      if (score === null) continue;
      const sectionBias = command.section === "accounts" ? 4 : command.section === "actions" ? 2 : 0;
      scored.push({ command, score: score + sectionBias });
    }
    scored.sort((a, b) => b.score - a.score || a.command.title.localeCompare(b.command.title));
    return scored.map((entry) => entry.command);
  });

  // Rows interleave section headers with commands, in section order when
  // idle and in score order (grouped) when searching.
  let rows = $derived.by(() => {
    const out: Array<{ header?: PaletteSection; command?: PaletteCommand; index?: number }> = [];
    let index = 0;
    for (const section of SECTION_ORDER) {
      const sectionCommands = filtered.filter((c) => c.section === section);
      if (sectionCommands.length === 0) continue;
      out.push({ header: section });
      for (const command of sectionCommands) {
        out.push({ command, index });
        index += 1;
      }
    }
    return out;
  });

  let flatCommands = $derived(rows.filter((row) => row.command).map((row) => row.command!));

  $effect(() => {
    void filtered;
    activeIndex = 0;
  });

  $effect(() => {
    inputRef?.focus();
  });

  function scrollActiveIntoView() {
    requestAnimationFrame(() => {
      listRef
        ?.querySelector(`#palette-item-${activeIndex}`)
        ?.scrollIntoView({ block: "nearest" });
    });
  }

  function moveActive(delta: number) {
    const count = flatCommands.length;
    if (count === 0) return;
    activeIndex = (activeIndex + delta + count) % count;
    scrollActiveIntoView();
  }

  function runCommand(command: PaletteCommand) {
    if (command.disabled) return;
    onClose();
    void command.run();
  }

  function handleInputKeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      e.stopPropagation();
      moveActive(1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      e.stopPropagation();
      moveActive(-1);
    } else if (e.key === "Home" && !query) {
      e.preventDefault();
      activeIndex = 0;
      scrollActiveIntoView();
    } else if (e.key === "End" && !query) {
      e.preventDefault();
      activeIndex = Math.max(0, flatCommands.length - 1);
      scrollActiveIntoView();
    } else if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      const command = flatCommands[activeIndex];
      if (command) runCommand(command);
    }
  }
</script>

<div
  class="palette-overlay"
  onmousedown={(e) => {
    if (e.target === e.currentTarget) onClose();
  }}
  role="presentation"
>
  <div class="palette" role="dialog" aria-label={t("palette.placeholder")}>
    <input
      bind:this={inputRef}
      bind:value={query}
      class="palette-input"
      type="text"
      placeholder={t("palette.placeholder")}
      role="combobox"
      aria-expanded="true"
      aria-controls="palette-list"
      aria-activedescendant={flatCommands.length > 0 ? `palette-item-${activeIndex}` : undefined}
      onkeydown={handleInputKeydown}
      spellcheck="false"
      autocomplete="off"
    />
    <div class="palette-list" id="palette-list" role="listbox" bind:this={listRef}>
      {#if flatCommands.length === 0}
        <div class="palette-empty">{t("palette.noResults")}</div>
      {:else}
        {#each rows as row (row.header ?? row.command!.id)}
          {#if row.header}
            <div class="palette-section">{t(SECTION_LABEL_KEYS[row.header])}</div>
          {:else if row.command}
            {@const command = row.command}
            <button
              type="button"
              class="palette-item"
              class:active={row.index === activeIndex}
              class:disabled={command.disabled}
              id={`palette-item-${row.index}`}
              role="option"
              aria-selected={row.index === activeIndex}
              aria-disabled={command.disabled || undefined}
              onmouseenter={() => {
                activeIndex = row.index!;
              }}
              onclick={() => runCommand(command)}
            >
              {#if command.accent}
                <span class="dot" style={`background:${command.accent};`} aria-hidden="true"></span>
              {/if}
              <span class="title">{command.title}</span>
              {#if command.active}
                <span class="badge">{t("common.active")}</span>
              {/if}
              {#if command.hint}
                <span class="hint">{command.hint}</span>
              {/if}
            </button>
          {/if}
        {/each}
      {/if}
    </div>
  </div>
</div>

<style>
  .palette-overlay {
    position: fixed;
    inset: 0;
    z-index: 1100;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    background: color-mix(in srgb, var(--bg) 45%, transparent);
    backdrop-filter: blur(3px);
    animation: overlay-fade-in 120ms ease-out;
  }

  .palette {
    margin-top: 12vh;
    width: min(520px, calc(100vw - 48px));
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 18px 48px color-mix(in srgb, var(--bg) 60%, transparent);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    animation: dialog-slide-in 140ms ease-out;
  }

  :global(html[data-motion="reduced"]) .palette-overlay,
  :global(html[data-motion="reduced"]) .palette {
    animation: none;
  }

  .palette-input {
    border: none;
    border-bottom: 1px solid var(--border);
    background: transparent;
    color: var(--fg);
    font-size: 14px;
    padding: 13px 16px;
    outline: none;
  }

  .palette-input::placeholder {
    color: var(--fg-subtle);
  }

  .palette-list {
    max-height: 46vh;
    overflow-y: auto;
    padding: 6px;
  }

  .palette-section {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--fg-subtle);
    padding: 8px 10px 4px;
  }

  .palette-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    border: none;
    background: transparent;
    color: var(--fg);
    font-size: 13px;
    padding: 8px 10px;
    border-radius: 6px;
    cursor: pointer;
  }

  .palette-item.active {
    background: color-mix(in srgb, var(--fg) 9%, transparent);
  }

  .palette-item.disabled {
    opacity: 0.45;
    cursor: default;
  }

  .palette-item .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .palette-item .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .palette-item .badge {
    font-size: 10px;
    font-weight: 600;
    color: var(--fg-muted);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 1px 7px;
    flex-shrink: 0;
  }

  .palette-item .hint {
    font-size: 11px;
    color: var(--fg-subtle);
    flex-shrink: 0;
  }

  .palette-empty {
    padding: 22px 12px;
    text-align: center;
    color: var(--fg-muted);
    font-size: 13px;
  }
</style>
