import type { PlatformAccount } from "$lib/shared/platform";
import type { PlatformDef } from "$lib/shared/platform";
import type { FolderInfo } from "$lib/features/folders/types";
import type { MessageKey, TranslationParams } from "$lib/i18n";

export type PaletteSection = "accounts" | "actions" | "navigation";

export type PaletteCommand = {
  id: string;
  section: PaletteSection;
  title: string;
  /** Extra matching terms (both locales) so "paramètres" finds Settings in EN. */
  keywords?: string[];
  /** Right-aligned hint, typically the shortcut ("Ctrl+R"). */
  hint?: string;
  /** Small color dot (platform/account accent). */
  accent?: string;
  /** Marks the current account / active tab. */
  active?: boolean;
  disabled?: boolean;
  run: () => void | Promise<void>;
};

type RegistryDeps = {
  t: (key: MessageKey, params?: TranslationParams) => string;
  getAccounts: () => PlatformAccount[];
  getCurrentAccountId: () => string | null;
  getEnabledPlatforms: () => PlatformDef[];
  getUnavailablePlatformIds: () => Set<string>;
  getActiveTab: () => string;
  getActiveTabUsable: () => boolean;
  getCurrentFolders: () => FolderInfo[];
  getCurrentFolderId: () => string | null;
  isBulkEditAvailable: () => boolean;
  isPersonasEnabled: () => boolean;
  getUpdateCtaLabel: () => string;
  getViewMode: () => string;
  isMac: () => boolean;
  switchToAccount: (account: PlatformAccount) => void;
  addAccount: () => void;
  refreshAccounts: () => void;
  newFolder: () => void;
  openFolder: (folderId: string) => void;
  navigateToParent: () => void;
  changeTab: (tab: string) => void;
  toggleSettings: () => void;
  openPersonas: () => void;
  toggleBulkEdit: () => void;
  toggleViewMode: () => void;
  zoomReset: () => void;
  applyUpdate: () => void;
};

function mod(isMac: boolean): string {
  return isMac ? "Cmd" : "Ctrl";
}

export function createCommandRegistry(deps: RegistryDeps) {
  function accountCommands(): PaletteCommand[] {
    const currentId = deps.getCurrentAccountId();
    return deps.getAccounts().map((account) => ({
      id: `account:${account.id}`,
      section: "accounts" as const,
      title: account.displayName || account.username || account.id,
      keywords: [account.username, account.id].filter(Boolean),
      active: account.id === currentId,
      hint: account.id === currentId ? deps.t("common.active") : undefined,
      run: () => deps.switchToAccount(account),
    }));
  }

  function actionCommands(): PaletteCommand[] {
    const m = mod(deps.isMac());
    const usable = deps.getActiveTabUsable();
    const commands: PaletteCommand[] = [
      {
        id: "action:add-account",
        section: "actions",
        title: deps.t("titlebar.addAccount"),
        keywords: ["add account", "ajouter un compte", "nouveau compte", "new account"],
        hint: `${m}+N`,
        disabled: !usable,
        run: deps.addAccount,
      },
      {
        id: "action:refresh",
        section: "actions",
        title: deps.t("titlebar.refresh"),
        keywords: ["refresh", "reload", "rafraichir", "actualiser"],
        hint: `${m}+R`,
        disabled: !usable,
        run: deps.refreshAccounts,
      },
      {
        id: "action:new-folder",
        section: "actions",
        title: deps.t("context.menu.newFolder"),
        keywords: ["new folder", "nouveau dossier", "creer dossier"],
        hint: `${m}+Shift+N`,
        run: deps.newFolder,
      },
      {
        id: "action:toggle-view",
        section: "actions",
        title: deps.t(deps.getViewMode() === "grid" ? "view.list" : "view.grid"),
        keywords: ["view", "grid", "list", "vue", "grille", "liste"],
        hint: `${m}+Shift+L`,
        run: deps.toggleViewMode,
      },
      {
        id: "action:zoom-reset",
        section: "actions",
        title: deps.t("palette.zoomReset"),
        keywords: ["zoom", "scale", "taille", "100%"],
        hint: `${m}+0`,
        run: deps.zoomReset,
      },
    ];
    if (deps.isBulkEditAvailable()) {
      commands.push({
        id: "action:bulk-edit",
        section: "actions",
        title: deps.t("bulkEdit.title"),
        keywords: ["bulk edit", "selection", "edition groupee", "multi"],
        hint: `${m}+E`,
        run: deps.toggleBulkEdit,
      });
    }
    if (deps.isPersonasEnabled()) {
      commands.push({
        id: "action:personas",
        section: "actions",
        title: deps.t("titlebar.personas"),
        keywords: ["personas", "profiles", "profils"],
        hint: `${m}+Shift+P`,
        run: deps.openPersonas,
      });
    }
    if (deps.getUpdateCtaLabel()) {
      commands.push({
        id: "action:apply-update",
        section: "actions",
        title: deps.t("update.restartToApply"),
        keywords: ["update", "mise a jour", "restart", "redemarrer"],
        run: deps.applyUpdate,
      });
    }
    return commands;
  }

  function navigationCommands(): PaletteCommand[] {
    const m = mod(deps.isMac());
    const activeTab = deps.getActiveTab();
    const unavailable = deps.getUnavailablePlatformIds();
    const commands: PaletteCommand[] = deps.getEnabledPlatforms().map((platform, index) => ({
      id: `nav:tab:${platform.id}`,
      section: "navigation" as const,
      title: deps.t("palette.openTab", { platform: platform.name }),
      keywords: [platform.name, platform.id],
      hint: index < 9 ? `${m}+${index + 1}` : undefined,
      accent: platform.accent,
      active: platform.id === activeTab,
      disabled: unavailable.has(platform.id),
      run: () => deps.changeTab(platform.id),
    }));
    for (const folder of deps.getCurrentFolders()) {
      commands.push({
        id: `nav:folder:${folder.id}`,
        section: "navigation",
        title: deps.t("palette.openFolder", { name: folder.name }),
        keywords: [folder.name, "folder", "dossier"],
        run: () => deps.openFolder(folder.id),
      });
    }
    if (deps.getCurrentFolderId()) {
      commands.push({
        id: "nav:parent",
        section: "navigation",
        title: deps.t("palette.backToParent"),
        keywords: ["back", "parent", "retour", "dossier parent"],
        hint: "Alt+←",
        run: deps.navigateToParent,
      });
    }
    commands.push({
      id: "nav:settings",
      section: "navigation",
      title: deps.t("titlebar.settings"),
      keywords: ["settings", "preferences", "parametres", "options", "reglages"],
      hint: `${m}+,`,
      run: deps.toggleSettings,
    });
    return commands;
  }

  function getCommands(): PaletteCommand[] {
    return [...accountCommands(), ...actionCommands(), ...navigationCommands()];
  }

  return { getCommands };
}
