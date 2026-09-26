import { applyThemeToDocument, themeUsesLiquidGlass } from "$lib/theme/themes";
import { applyWindowBackdrop } from "$lib/theme/backdrop";
import { applyMotionPreference } from "$lib/theme/motion";
import type { AnimationsMode } from "$lib/features/settings/types";
import type { createPlatformShellState } from "./platformShell.svelte";
import { createLiquidBackdrop } from "./useLiquidBackdrop.svelte";

type PlatformShell = ReturnType<typeof createPlatformShellState>;

type ThemeApplicationDeps = {
  shell: Pick<PlatformShell, "activeTheme" | "runtimeOs" | "settings" | "locale">;
  getAnimations: () => AnimationsMode;
};

/**
 * Keeps the document in step with the theme, the locale and the motion
 * setting, and runs the Liquid Glass wallpaper backdrop on Windows. Create
 * during component init: it registers its effects there.
 */
export function createThemeApplication({ shell, getAnimations }: ThemeApplicationDeps) {
  const liquidBackdropActive = $derived(
    themeUsesLiquidGlass(shell.activeTheme) && shell.runtimeOs === "windows",
  );
  const liquidBackdrop = createLiquidBackdrop({ isActive: () => liquidBackdropActive });

  $effect(() => {
    const backdropAvailable =
      shell.runtimeOs !== "linux" && (!liquidBackdropActive || liquidBackdrop.wallpaper !== null);
    applyThemeToDocument(shell.activeTheme, shell.settings.backgroundOpacity, document, {
      // Linux compositors expose no portable blur-behind protocol; glass
      // themes degrade to a near-solid window there. Liquid Glass does the
      // same on Windows until a real wallpaper snapshot is available.
      backdropAvailable,
    });
    document.documentElement.lang = shell.locale;
    document.documentElement.dataset.cardOutlines = shell.settings.accountDisplay.cardColorOutlines
      ? "1"
      : "0";
    // Glass themes need the OS backdrop blur to read as glass.
    void applyWindowBackdrop(
      Boolean(shell.activeTheme.glass),
      shell.activeTheme.id,
      themeUsesLiquidGlass(shell.activeTheme),
    );
  });

  $effect(() => applyMotionPreference(getAnimations()));

  return {
    get liquidBackdropActive() {
      return liquidBackdropActive;
    },
    liquidBackdrop,
  };
}
