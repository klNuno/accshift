import type { MessageKey } from "$lib/i18n";

export type SettingsTabDef = {
  id: string;
  labelKey: MessageKey;
  accent: string;
  visible?: () => boolean;
};

type TabBarDeps = {
  getVisibleTabs: () => SettingsTabDef[];
  onTabSelected: (tabId: string) => void;
};

export function createSettingsTabBar(deps: TabBarDeps) {
  let activeTab = $state<string>("general");
  let tabsRef = $state<HTMLDivElement | null>(null);
  let tabUiFrame: number | null = null;

  function scrollActiveIntoView(behavior: ScrollBehavior = "smooth") {
    const btn = tabsRef?.querySelector<HTMLElement>(`[data-settings-tab="${activeTab}"]`);
    btn?.scrollIntoView({ inline: "nearest", block: "nearest", behavior });
  }

  function queueScrollActive() {
    if (tabUiFrame !== null) cancelAnimationFrame(tabUiFrame);
    tabUiFrame = requestAnimationFrame(() => {
      scrollActiveIntoView("auto");
      tabUiFrame = null;
    });
  }

  function select(tabId: string) {
    activeTab = tabId;
    deps.onTabSelected(tabId);
    queueScrollActive();
  }

  function ensureActiveVisible() {
    const visibleIds = deps.getVisibleTabs().map((t) => t.id);
    if (!visibleIds.includes(activeTab)) {
      activeTab = visibleIds[0] ?? "general";
      deps.onTabSelected(activeTab);
    }
    queueScrollActive();
  }

  function destroy() {
    if (tabUiFrame !== null) cancelAnimationFrame(tabUiFrame);
  }

  return {
    get activeTab() {
      return activeTab;
    },
    get tabsRef() {
      return tabsRef;
    },
    set tabsRef(v: HTMLDivElement | null) {
      tabsRef = v;
    },
    select,
    ensureActiveVisible,
    destroy,
  };
}
