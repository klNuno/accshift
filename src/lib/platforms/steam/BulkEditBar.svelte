<script lang="ts">
  import type { MessageKey, TranslationParams } from "$lib/i18n";
  import type { CopyableGame } from "./types";
  import {
    getAccountGames,
    bulkEdit,
    type LaunchOptionEdit,
    type BulkEditResult,
  } from "./steamApi";

  type TriState = "unchanged" | "enabled" | "disabled";

  let {
    selectedIds,
    activeAccountSelected = false,
    onSelectAll,
    onDeselectAll,
    onClose,
    onResult,
    t,
  }: {
    selectedIds: Set<string>;
    activeAccountSelected?: boolean;
    onSelectAll: () => void;
    onDeselectAll: () => void;
    onClose: () => void;
    onResult: (result: BulkEditResult) => void;
    t: (key: MessageKey, params?: TranslationParams) => string;
  } = $props();

  let step = $state<"select" | "settings">("select");
  let newsPopup = $state<TriState>("unchanged");
  let dnd = $state<TriState>("unchanged");
  let launchOptionEdits = $state<LaunchOptionEdit[]>([]);
  let games = $state<CopyableGame[]>([]);
  let gamesLoading = $state(false);
  let selectedGameId = $state("");
  let launchOptionInput = $state("");
  let applying = $state(false);

  function cycleTriState(current: TriState): TriState {
    if (current === "unchanged") return "enabled";
    if (current === "enabled") return "disabled";
    return "unchanged";
  }

  function triStateLabel(state: TriState): string {
    if (state === "unchanged") return t("bulkEdit.unchanged");
    if (state === "enabled") return t("common.enabled");
    return t("common.disabled");
  }

  function triStateToValue(state: TriState): boolean | null {
    if (state === "enabled") return true;
    if (state === "disabled") return false;
    return null;
  }

  function gameName(appId: string): string {
    return games.find((g) => g.app_id === appId)?.name ?? appId;
  }

  async function goToSettings() {
    step = "settings";
    if (games.length === 0 && selectedIds.size > 0) {
      gamesLoading = true;
      try {
        games = await getAccountGames([...selectedIds][0]);
      } catch {
        games = [];
      } finally {
        gamesLoading = false;
      }
    }
  }

  function addLaunchOption() {
    if (!selectedGameId || !launchOptionInput.trim()) return;
    launchOptionEdits = [
      ...launchOptionEdits.filter((e) => e.appId !== selectedGameId),
      { appId: selectedGameId, value: launchOptionInput.trim() },
    ];
    selectedGameId = "";
    launchOptionInput = "";
  }

  function removeLaunchOption(appId: string) {
    launchOptionEdits = launchOptionEdits.filter((e) => e.appId !== appId);
  }

  async function applyEdits() {
    applying = true;
    try {
      const result = await bulkEdit({
        steamIds: [...selectedIds],
        newsPopup: triStateToValue(newsPopup),
        doNotDisturb: triStateToValue(dnd),
        launchOptions: launchOptionEdits,
      });
      onResult(result);
      onClose();
    } catch (e) {
      onResult({ succeeded: 0, failed: [{ steamId: "all", error: String(e) }] });
      onClose();
    } finally {
      applying = false;
    }
  }

  function handleOverlayKeydown(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    if (step === "settings") step = "select";
    else onClose();
  }
</script>

<svelte:window onkeydown={handleOverlayKeydown} />

{#if step === "select"}
  <div class="bar">
    <div class="bar-left">
      <button class="tool-btn" onclick={onSelectAll}>{t("bulkEdit.selectAll")}</button>
      <button class="tool-btn" onclick={onDeselectAll}>{t("bulkEdit.deselectAll")}</button>
      <span class="count">{t("bulkEdit.selected", { count: selectedIds.size })}</span>
    </div>
    <div class="bar-right">
      <button class="btn-secondary" onclick={onClose}>{t("common.cancel")}</button>
      <button class="btn-primary" disabled={selectedIds.size === 0} onclick={goToSettings}>
        {t("bulkEdit.next")}
      </button>
    </div>
  </div>
{:else if step === "settings"}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="overlay" onclick={() => (step = "select")}>
    <div class="dialog" onclick={(e) => e.stopPropagation()}>
        <div class="dialog-header">
          <span class="dialog-title">{t("bulkEdit.settings")}</span>
          <span class="dialog-count">{t("bulkEdit.selected", { count: selectedIds.size })}</span>
        </div>

        <div class="settings-body">
          <div class="tri-state-row">
            <span class="setting-label">{t("bulkEdit.newsPopup")}</span>
            <button
              class="tri-state-btn"
              class:active={newsPopup !== "unchanged"}
              onclick={() => (newsPopup = cycleTriState(newsPopup))}
            >
              {triStateLabel(newsPopup)}
            </button>
          </div>

          <div class="tri-state-row">
            <span class="setting-label">{t("bulkEdit.doNotDisturb")}</span>
            <button
              class="tri-state-btn"
              class:active={dnd !== "unchanged"}
              onclick={() => (dnd = cycleTriState(dnd))}
            >
              {triStateLabel(dnd)}
            </button>
          </div>

          <div class="launch-section">
            <span class="setting-label">{t("bulkEdit.launchOptions")}</span>
            {#if gamesLoading}
              <span class="hint">{t("bulkEdit.loadingGames")}</span>
            {:else}
              <div class="launch-add-row">
                <select class="game-select" bind:value={selectedGameId}>
                  <option value="">{t("bulkEdit.selectGame")}</option>
                  {#each games as game (game.app_id)}
                    <option value={game.app_id}>{game.name}</option>
                  {/each}
                </select>
                <input
                  class="launch-input"
                  type="text"
                  bind:value={launchOptionInput}
                  placeholder="-fullscreen"
                  onkeydown={(e) => { if (e.key === "Enter") addLaunchOption(); }}
                />
                <button
                  class="tool-btn"
                  onclick={addLaunchOption}
                  disabled={!selectedGameId || !launchOptionInput.trim()}
                >
                  {t("bulkEdit.addLaunchOption")}
                </button>
              </div>
            {/if}

            {#if launchOptionEdits.length > 0}
              <div class="launch-list">
                {#each launchOptionEdits as edit (edit.appId)}
                  <div class="launch-item">
                    <span class="launch-game">{gameName(edit.appId)}</span>
                    <code class="launch-value">{edit.value}</code>
                    <button class="remove-btn" onclick={() => removeLaunchOption(edit.appId)}>&times;</button>
                  </div>
                {/each}
              </div>
            {/if}
          </div>

          {#if activeAccountSelected}
            <div class="warning">{t("bulkEdit.activeAccountWarning")}</div>
          {/if}
        </div>

        <div class="dialog-actions">
          <button class="btn-secondary" onclick={() => (step = "select")}>{t("common.back")}</button>
          <button class="btn-primary" disabled={applying} onclick={applyEdits}>
            {applying ? "..." : t("bulkEdit.apply")}
          </button>
        </div>
    </div>
  </div>
{/if}

<style>
  /* ── Bottom bar (select step) ── */

  .bar {
    position: fixed;
    bottom: 0;
    left: 0;
    right: 0;
    z-index: 100;
    background: var(--bg-overlay);
    border-top: 1px solid var(--border);
    padding: 8px 16px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    animation: slideUp 120ms ease-out;
  }

  @keyframes slideUp {
    from { opacity: 0; transform: translateY(100%); }
    to { opacity: 1; transform: translateY(0); }
  }

  .bar-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .bar-right {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .count {
    font-size: 12px;
    color: var(--fg-muted);
  }

  .tool-btn {
    padding: 4px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-card);
    color: var(--fg-muted);
    font-size: 11px;
    cursor: pointer;
  }

  .tool-btn:hover:not(:disabled) {
    background: var(--bg-card-hover);
    color: var(--fg);
  }

  .tool-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-secondary {
    padding: 5px 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: transparent;
    color: var(--fg-muted);
    font-size: 12px;
    cursor: pointer;
    transition: all 100ms;
  }

  .btn-secondary:hover {
    background: var(--bg-muted);
    color: var(--fg);
  }

  .btn-primary {
    padding: 5px 12px;
    border: 1px solid transparent;
    border-radius: 6px;
    background: #2563eb;
    color: #fff;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: all 100ms;
  }

  .btn-primary:hover:not(:disabled) {
    background: #1d4ed8;
  }

  .btn-primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  /* ── Overlay (settings + result) ── */

  .overlay {
    position: fixed;
    inset: 0;
    z-index: 1100;
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
    width: min(440px, calc(100vw - 24px));
    padding: 16px;
    background: var(--bg-overlay);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.4);
    display: flex;
    flex-direction: column;
    gap: 14px;
    animation: slideIn 120ms ease-out;
  }

  @keyframes slideIn {
    from { opacity: 0; transform: scale(0.96) translateY(8px); }
    to { opacity: 1; transform: scale(1) translateY(0); }
  }

  .dialog-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .dialog-title {
    font-size: 13px;
    font-weight: 650;
    color: var(--fg);
  }

  .dialog-count {
    font-size: 11px;
    color: var(--fg-muted);
  }

  .dialog-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  /* ── Settings body ── */

  .settings-body {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .tri-state-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 2px 0;
  }

  .setting-label {
    font-size: 12px;
    color: var(--fg);
  }

  .tri-state-btn {
    padding: 4px 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-card);
    color: var(--fg-muted);
    font-size: 11px;
    cursor: pointer;
    min-width: 86px;
    text-align: center;
    transition: all 100ms;
  }

  .tri-state-btn:hover {
    background: var(--bg-card-hover);
  }

  .tri-state-btn.active {
    border-color: color-mix(in srgb, #2563eb 50%, var(--border));
    color: var(--fg);
  }

  .launch-section {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .hint {
    font-size: 11px;
    color: var(--fg-subtle);
  }

  .launch-add-row {
    display: flex;
    gap: 6px;
  }

  .game-select {
    flex: 1;
    min-width: 0;
    padding: 5px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-solid);
    color: var(--fg);
    font-size: 11px;
  }

  .launch-input {
    width: 120px;
    padding: 5px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-solid);
    color: var(--fg);
    font-size: 11px;
    outline: none;
  }

  .launch-input:focus {
    border-color: color-mix(in srgb, var(--fg-muted) 55%, var(--border));
  }

  .launch-list {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .launch-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 6px;
    font-size: 11px;
  }

  .launch-game {
    color: var(--fg);
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 160px;
  }

  .launch-value {
    flex: 1;
    min-width: 0;
    color: var(--fg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .remove-btn {
    border: none;
    background: transparent;
    color: var(--fg-muted);
    font-size: 14px;
    cursor: pointer;
    padding: 0 2px;
    line-height: 1;
    flex-shrink: 0;
  }

  .remove-btn:hover {
    color: #ef4444;
  }

  .warning {
    font-size: 11px;
    color: #f59e0b;
    padding: 6px 10px;
    border: 1px solid color-mix(in srgb, #f59e0b 30%, var(--border));
    border-radius: 6px;
    background: color-mix(in srgb, #f59e0b 8%, var(--bg-card));
  }

</style>
