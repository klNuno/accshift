import { describe, expect, it } from "vitest";
import { diffSettings, mergeSettingsDraft } from "./settingsPatch";
import type { AppSettings } from "./types";

function settings(overrides: Partial<AppSettings> = {}): AppSettings {
  return {
    language: "en",
    themeId: "dark",
    backgroundOpacity: 100,
    uiScalePercent: 100,
    animations: "system",
    streamerMode: "auto",
    suspendGraphicsWhenMinimized: true,
    minimizeOnAccountSwitch: false,
    dataRefresh: { avatarCacheDays: 7, banCheckDays: 7 },
    enabledPlatforms: ["steam", "riot"],
    healthCheckPerPlatform: {},
    personasEnabled: true,
    defaultPlatformId: "steam",
    inactivityBlurSeconds: 60,
    deepLinksEnabled: true,
    cliEnabled: true,
    platformSettings: {
      steam: { runAsAdmin: false, launchOptions: "", shutdownMode: "graceful" },
    } as unknown as AppSettings["platformSettings"],
    accountDisplay: {
      showUsernames: true,
      showLastLoginPerPlatform: {},
      showCardNotesInline: false,
      expandedFolders: false,
      cardColorOutlines: true,
    },
    pinEnabled: true,
    pinHash: "legacy-hash",
    ...overrides,
  };
}

describe("mergeSettingsDraft", () => {
  it("keeps a zoom change made elsewhere while the panel was open", () => {
    // Settings opened at 100%, ctrl+plus twice took the store to 120%, then
    // the user flipped a toggle in the panel. The zoom must stay at 120%.
    const baseline = settings();
    const current = settings({ uiScalePercent: 120 });
    const draft = settings({ minimizeOnAccountSwitch: true });

    const merged = mergeSettingsDraft(current, baseline, draft);

    expect(merged.uiScalePercent).toBe(120);
    expect(merged.minimizeOnAccountSwitch).toBe(true);
  });

  it("keeps the streamer flag and a rehashed PIN written elsewhere", () => {
    const baseline = settings();
    const current = settings({ streamerMode: "off", pinHash: "upgraded-hash" });
    const draft = settings({ themeId: "light" });

    const merged = mergeSettingsDraft(current, baseline, draft);

    expect(merged.streamerMode).toBe("off");
    expect(merged.pinHash).toBe("upgraded-hash");
    expect(merged.themeId).toBe("light");
  });

  it("merges nested fields one leaf at a time", () => {
    const baseline = settings();
    const current = settings({
      accountDisplay: { ...settings().accountDisplay, expandedFolders: true },
    });
    const draft = settings({
      accountDisplay: { ...settings().accountDisplay, showUsernames: false },
    });

    const merged = mergeSettingsDraft(current, baseline, draft);

    expect(merged.accountDisplay.expandedFolders).toBe(true);
    expect(merged.accountDisplay.showUsernames).toBe(false);
  });

  it("replaces an edited list whole", () => {
    const baseline = settings();
    const current = settings({ enabledPlatforms: ["steam", "riot", "roblox"] });
    const draft = settings({ enabledPlatforms: ["riot"], defaultPlatformId: "riot" });

    const merged = mergeSettingsDraft(current, baseline, draft);

    expect(merged.enabledPlatforms).toEqual(["riot"]);
    expect(merged.defaultPlatformId).toBe("riot");
  });

  it("applies the user's own edit over a different value written elsewhere", () => {
    const merged = mergeSettingsDraft(
      settings({ uiScalePercent: 120 }),
      settings(),
      settings({ uiScalePercent: 90 }),
    );
    expect(merged.uiScalePercent).toBe(90);
  });

  it("leaves the current settings object untouched", () => {
    const current = settings();
    mergeSettingsDraft(current, settings(), settings({ language: "fr" }));
    expect(current.language).toBe("en");
  });

  it("rebases an open panel on another writer's values and keeps its own edits", () => {
    // The panel opened on `baseline`, the user picked French, then a zoom
    // shortcut stored 110 while the panel was open.
    const baseline = settings();
    const draft = settings({ language: "fr" });
    const stored = settings({ uiScalePercent: 110 });

    const shown = mergeSettingsDraft(stored, baseline, draft);
    expect(shown.uiScalePercent).toBe(110);
    expect(shown.language).toBe("fr");

    // With the stored settings as the new baseline, the next save writes
    // only the language and keeps the zoom.
    const saved = mergeSettingsDraft(stored, stored, shown);
    expect(saved).toEqual(settings({ language: "fr", uiScalePercent: 110 }));
    expect(diffSettings(stored, shown)).toEqual([{ path: ["language"], value: "fr" }]);
  });
});

describe("diffSettings", () => {
  it("reports nothing when the draft matches the baseline", () => {
    expect(diffSettings(settings(), settings())).toEqual([]);
  });

  it("reports the path of each changed leaf", () => {
    const changes = diffSettings(
      settings(),
      settings({
        dataRefresh: { avatarCacheDays: 3, banCheckDays: 7 },
        enabledPlatforms: ["steam"],
      }),
    );
    expect(changes).toEqual([
      { path: ["dataRefresh", "avatarCacheDays"], value: 3 },
      { path: ["enabledPlatforms"], value: ["steam"] },
    ]);
  });
});
