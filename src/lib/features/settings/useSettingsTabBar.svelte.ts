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
  let tabResizeObserver: ResizeObserver | null = null;
  let tabsOverflowing = $state(false);
  let canScrollLeft = $state(false);
  let canScrollRight = $state(false);

  function updateScrollState() {
    const el = tabsRef;
    if (!el) {
      tabsOverflowing = false;
      canScrollLeft = false;
      canScrollRight = false;
      return;
    }
    const maxScrollLeft = Math.max(0, el.scrollWidth - el.clientWidth);
    tabsOverflowing = maxScrollLeft > 6;
    canScrollLeft = tabsOverflowing && el.scrollLeft > 6;
    canScrollRight = tabsOverflowing && el.scrollLeft < maxScrollLeft - 6;
  }

  function scrollActiveIntoView(behavior: ScrollBehavior = "smooth") {
    const btn = tabsRef?.querySelector<HTMLElement>(`[data-settings-tab="${activeTab}"]`);
    btn?.scrollIntoView({ inline: "nearest", block: "nearest", behavior });
  }

  function queueUiRefresh(scrollActive = false) {
    if (tabUiFrame !== null) cancelAnimationFrame(tabUiFrame);
    tabUiFrame = requestAnimationFrame(() => {
      updateScrollState();
      if (scrollActive) scrollActiveIntoView("auto");
      tabUiFrame = null;
    });
  }

  function select(tabId: string) {
    activeTab = tabId;
    deps.onTabSelected(tabId);
    queueUiRefresh(true);
  }

  function scroll(direction: -1 | 1) {
    const el = tabsRef;
    if (!el) return;
    el.scrollBy({
      left: Math.max(180, el.clientWidth * 0.6) * direction,
      behavior: "smooth",
    });
  }

  function ensureActiveVisible() {
    const visibleIds = deps.getVisibleTabs().map((t) => t.id);
    if (!visibleIds.includes(activeTab)) {
      activeTab = visibleIds[0] ?? "general";
      deps.onTabSelected(activeTab);
    }
    queueUiRefresh(true);
  }

  function startObserver() {
    if (tabsRef && typeof ResizeObserver !== "undefined") {
      tabResizeObserver = new ResizeObserver(() => updateScrollState());
      tabResizeObserver.observe(tabsRef);
    }
    queueUiRefresh(true);
  }

  function destroy() {
    if (tabUiFrame !== null) cancelAnimationFrame(tabUiFrame);
    tabResizeObserver?.disconnect();
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
    get tabsOverflowing() {
      return tabsOverflowing;
    },
    get canScrollLeft() {
      return canScrollLeft;
    },
    get canScrollRight() {
      return canScrollRight;
    },
    select,
    scroll,
    updateScrollState,
    ensureActiveVisible,
    startObserver,
    destroy,
  };
}
