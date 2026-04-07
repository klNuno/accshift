(() => {
  const DARK = {
    bg: "#09090b",
    fg: "#fafafa",
    panel: "rgba(24, 24, 27, 0.92)",
    border: "rgba(63, 63, 70, 0.65)",
    glow: "rgba(59, 130, 246, 0.14)",
    muted: "#71717a",
  };
  const LIGHT = {
    bg: "#f1f1f3",
    fg: "#0b0b0f",
    panel: "rgba(255, 255, 255, 0.88)",
    border: "rgba(184, 184, 197, 0.75)",
    glow: "rgba(59, 130, 246, 0.10)",
    muted: "#4e4e5d",
  };

  let theme = "dark";
  try {
    const bootTheme = localStorage.getItem("accshift_boot_theme");
    if (bootTheme) {
      const bt = JSON.parse(bootTheme);
      if (bt?.colorScheme === "light") theme = "light";
    }
  } catch {}

  const palette = theme === "light" ? LIGHT : DARK;
  const root = document.documentElement;
  root.dataset.theme = theme;
  root.style.colorScheme = theme;
  root.style.setProperty("--boot-bg", palette.bg);
  root.style.setProperty("--boot-fg", palette.fg);
  root.style.setProperty("--boot-panel", palette.panel);
  root.style.setProperty("--boot-border", palette.border);
  root.style.setProperty("--boot-glow", palette.glow);
  root.style.setProperty("--boot-muted", palette.muted);
})();
