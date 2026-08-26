import { invoke } from "@tauri-apps/api/core";
import type { Component, ComponentProps } from "svelte";
import type TelemetryOnboardingType from "$lib/features/settings/TelemetryOnboarding.svelte";
import type { ItemRef } from "$lib/features/folders/types";
import type { PlatformAccount } from "$lib/shared/platform";
import type { MessageKey } from "$lib/i18n";

type OnboardingTourDeps = {
  t: (key: MessageKey, params?: Record<string, string | number>) => string;
  getActiveTab: () => string;
  setActiveTab: (tab: string) => void;
};

/** Ids of the fake accounts, prefixed so nothing mistakes one for a real id. */
const MOCK_IDS = ["__tour_mock_1", "__tour_mock_2", "__tour_mock_3"];
/** The tour walks through Steam's card features, so it needs Steam on screen. */
const TOUR_PLATFORM = "steam";

/**
 * The first-run telemetry onboarding, and the fake account grid its feature
 * tour draws over.
 *
 * A brand new install has no accounts at all, which is exactly when the tour
 * needs cards to point at. It swaps three mock accounts into the workspace for
 * the duration and puts the previous tab back afterwards, so nothing about the
 * user's real state is touched.
 *
 * The onboarding component itself is imported on demand: most launches never
 * show it.
 */
export function createOnboardingTour({ t, getActiveTab, setActiveTab }: OnboardingTourDeps) {
  let open = $state(false);
  let component = $state<Component<ComponentProps<typeof TelemetryOnboardingType>> | null>(null);
  let mockActive = $state(false);
  let previousTab: string | null = null;

  const mockAccounts = $derived<PlatformAccount[]>(
    MOCK_IDS.map((id, index) => ({
      id,
      displayName: t("onboarding.features.mockAccount", { number: index + 1 }),
      username: `account_${index + 1}`,
      lastLoginAt: null,
    })),
  );
  const mockItems = $derived<ItemRef[]>(
    mockAccounts.map((account) => ({
      type: "account" as const,
      id: account.id,
    })),
  );
  const mockMap = $derived<Record<string, PlatformAccount>>(
    Object.fromEntries(mockAccounts.map((account) => [account.id, account])),
  );

  function activateMock() {
    previousTab = getActiveTab();
    if (getActiveTab() !== TOUR_PLATFORM) {
      setActiveTab(TOUR_PLATFORM);
    }
    mockActive = true;
  }

  function deactivateMock() {
    mockActive = false;
    if (previousTab && previousTab !== getActiveTab()) {
      setActiveTab(previousTab);
    }
    previousTab = null;
  }

  async function openOnboarding() {
    const onbModule = await import("$lib/features/settings/TelemetryOnboarding.svelte");
    component = onbModule.default as Component<ComponentProps<typeof TelemetryOnboardingType>>;
    open = true;
  }

  function close() {
    open = false;
    deactivateMock();
  }

  /** Show the onboarding on a first launch. A failed read shows nothing. */
  async function openIfNeverCompleted() {
    try {
      type TelemetryState = { onboarding_completed: boolean };
      const state = await invoke<TelemetryState>("telemetry_get_state");
      if (!state.onboarding_completed) {
        await openOnboarding();
      }
    } catch (e) {
      console.error("telemetry_get_state failed", e);
    }
  }

  return {
    get open() {
      return open;
    },
    /** Null until the onboarding is opened for the first time. */
    get component() {
      return component;
    },
    /** True while the workspace should show the tour's fake accounts. */
    get mockActive() {
      return mockActive;
    },
    get mockAccounts() {
      return mockAccounts;
    },
    get mockItems() {
      return mockItems;
    },
    get mockMap() {
      return mockMap;
    },
    openOnboarding,
    openIfNeverCompleted,
    setMockActive: (active: boolean) => (active ? activateMock() : deactivateMock()),
    close,
  };
}
