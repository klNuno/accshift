import type { PlatformAccount, PlatformAddFlowStatus } from "$lib/shared/platform";
import type { CardExtensionContent } from "$lib/shared/cardExtension";
import { getPlatform } from "$lib/shared/platform";
import type { MessageKey, TranslationParams } from "$lib/i18n";

type Translator = (key: MessageKey, params?: TranslationParams) => string;

type PlatformAddFlowEntry = {
  platformId: string;
  status: PlatformAddFlowStatus;
};

type PlatformAddFlowDeps = {
  getActiveTab: () => string;
  getCurrentFolderId: () => string | null;
  getIsSearching: () => boolean;
  t: Translator;
  showToast: (message: string) => void;
  copyToClipboard: (text: string) => void;
  loadAccounts: (
    silent?: boolean,
    showRefreshedToast?: boolean,
    forceRefresh?: boolean,
    checkBans?: boolean,
    deferBackground?: boolean,
  ) => void;
  onAccountAdded?: (platformId: string, accountId: string) => void;
};

function getSetupKey(
  platformId: string,
  kind:
    | "pendingLabel"
    | "waitingForClient"
    | "waitingForLogin"
    | "detected"
    | "connected"
    | "ready"
    | "readyWithProfile"
    | "failed"
    | "failedMessage",
): MessageKey {
  const PLATFORM_MESSAGE_PREFIX: Record<string, string> = {
    steam: "steam",
    riot: "riot",
    "battle-net": "battlenet",
    ubisoft: "ubisoft",
    roblox: "roblox",
  };
  const prefix = PLATFORM_MESSAGE_PREFIX[platformId] ?? platformId;
  return `${prefix}.setup${kind[0].toUpperCase()}${kind.slice(1)}` as MessageKey;
}

function createDetectedSection(
  platformId: string,
  display: string,
  t: Translator,
): CardExtensionContent["sections"][number] | null {
  if (!display) return null;
  return {
    title: t(getSetupKey(platformId, "detected")),
    text: display,
    chips: [{ text: t(getSetupKey(platformId, "connected")), tone: "green" as const }],
  };
}

export function createPlatformAddFlowController({
  getActiveTab,
  getCurrentFolderId,
  getIsSearching,
  t,
  showToast,
  copyToClipboard,
  loadAccounts,
  onAccountAdded,
}: PlatformAddFlowDeps) {
  let flow = $state<PlatformAddFlowEntry | null>(null);
  let timer: ReturnType<typeof setTimeout> | null = null;

  let pendingSetupAccount = $derived.by(() => {
    if (!flow) return null;
    if (getActiveTab() !== flow.platformId) return null;
    if (getIsSearching() || getCurrentFolderId()) return null;
    if (flow.status.state === "ready") return null;
    const setupId = flow.status.setupId.trim();
    if (!setupId) return null;
    const detectedName = (flow.status.accountDisplayName || "").trim();
    return {
      id: setupId,
      displayName: detectedName || t("platform.newAccountPending"),
      username: detectedName
        ? t(getSetupKey(flow.platformId, "connected"))
        : t(getSetupKey(flow.platformId, "waitingForLogin")),
      lastLoginAt: null,
    } satisfies PlatformAccount;
  });

  function clearTimer() {
    if (!timer) return;
    clearTimeout(timer);
    timer = null;
  }

  function stop() {
    clearTimer();
    flow = null;
  }

  async function cancel() {
    const current = flow;
    const flowAdapter = current ? getPlatform(current.platformId) : undefined;
    if (!current || !flowAdapter?.cancelAddFlow) {
      stop();
      return;
    }
    try {
      await flowAdapter.cancelAddFlow(current.status.setupId);
    } catch (error) {
      showToast(String(error));
    }
    stop();
    if (getActiveTab() === current.platformId) {
      loadAccounts(true);
    }
  }

  async function cancelIfConflicting(targetPlatformId: string, targetAccountId?: string) {
    const current = flow;
    if (!current) return;
    if (current.platformId !== targetPlatformId) return;
    if (targetAccountId && current.status.setupId === targetAccountId) return;
    await cancel();
  }

  function schedulePoll() {
    clearTimer();
    timer = setTimeout(() => {
      void poll();
    }, 1500);
  }

  async function poll() {
    const current = flow;
    const flowAdapter = current ? getPlatform(current.platformId) : undefined;
    if (!current || !flowAdapter?.pollAddFlow) {
      stop();
      return;
    }

    try {
      const nextStatus = await flowAdapter.pollAddFlow(current.status.setupId);
      if (!flow || flow.status.setupId !== current.status.setupId) return;
      flow = { ...flow, status: nextStatus };

      if (nextStatus.state === "ready") {
        const adapter = getPlatform(current.platformId);
        if (current.platformId === "steam" && nextStatus.accountId) {
          void adapter?.getProfileInfo?.(nextStatus.accountId).catch(() => null);
        }
        if (getActiveTab() === current.platformId) {
          loadAccounts(true, false, false, false, false);
        }
        showToast(
          nextStatus.accountDisplayName
            ? t(getSetupKey(current.platformId, "readyWithProfile"), {
                profile: nextStatus.accountDisplayName,
              })
            : t(getSetupKey(current.platformId, "ready")),
        );
        stop();
        if (adapter?.setAccountLabel && nextStatus.accountId) {
          onAccountAdded?.(current.platformId, nextStatus.accountId);
        }
        return;
      }

      if (nextStatus.state === "failed") {
        return;
      }

      schedulePoll();
    } catch (error) {
      if (!flow || flow.status.setupId !== current.status.setupId) return;
      flow = {
        ...flow,
        status: {
          ...flow.status,
          state: "failed",
          errorMessage: String(error),
        },
      };
    }
  }

  function start(platformId: string, status: PlatformAddFlowStatus) {
    flow = { platformId, status };
    if (status.state !== "ready" && status.state !== "failed") {
      schedulePoll();
    }
  }

  function getSetupExtensionContent(accountId: string): CardExtensionContent | null {
    if (!flow) return null;
    const setupId = flow.status.setupId.trim();
    if (!setupId || setupId !== accountId) return null;

    const display = (flow.status.accountDisplayName || "").trim();
    const error = (flow.status.errorMessage || "").trim();
    const detectedSection = createDetectedSection(flow.platformId, display, t);

    if (flow.platformId === "riot") {
      switch (flow.status.state) {
        case "waiting_for_client":
          return {
            sections: [
              {
                text: t("riot.setupWaitingForClient"),
                loading: true,
              },
              {
                lines: [t("riot.setupStaySignedIn")],
              },
            ],
          };
        case "capturing":
          return {
            sections: [
              {
                text: t("riot.setupCapturing"),
                loading: true,
              },
              ...(detectedSection ? [detectedSection] : []),
            ],
          };
        case "failed":
          return {
            sections: [
              {
                title: t("riot.setupFailed"),
                text: t("riot.setupFailedMessage"),
              },
              ...(error
                ? [
                    {
                      lines: [error],
                      chips: [{ text: t("common.close"), tone: "red" as const }],
                    },
                  ]
                : []),
            ],
          };
        case "ready":
          return null;
        case "waiting_for_login":
        default:
          return {
            sections: [
              {
                text: t("riot.setupWaitingForLogin"),
                loading: true,
              },
              ...(detectedSection ? [detectedSection] : [{ lines: [t("riot.setupStaySignedIn")] }]),
            ],
          };
      }
    }

    if (flow.platformId === "roblox") {
      switch (flow.status.state) {
        case "failed":
          return {
            sections: [
              {
                title: t("roblox.setupFailed"),
                text: error || t("roblox.setupFailedMessage"),
              },
            ],
          };
        case "ready":
          return null;
        case "waiting_for_client":
        case "waiting_for_login":
        default:
          return {
            sections: [
              {
                link: {
                  label: "roblox.com/quick-login",
                  url: "https://www.roblox.com/crossdevicelogin/ConfirmCode",
                },
                chips: [
                  { text: "Quick Login", tone: "green" as const },
                  ...(display
                    ? [
                        {
                          text: t("roblox.copyCode"),
                          tone: "slate" as const,
                          onClick: () => copyToClipboard(display),
                        },
                      ]
                    : []),
                ],
                loading: true,
              },
            ],
          };
      }
    }

    if (flow.platformId === "battle-net") {
      switch (flow.status.state) {
        case "waiting_for_client":
          return {
            sections: [
              {
                text: t("battlenet.setupWaitingForClient"),
                loading: true,
              },
            ],
          };
        case "failed":
          return {
            sections: [
              {
                title: t("battlenet.setupFailed"),
                text: t("battlenet.setupFailedMessage"),
              },
              ...(error
                ? [
                    {
                      lines: [error],
                      chips: [{ text: t("common.close"), tone: "red" as const }],
                    },
                  ]
                : []),
            ],
          };
        case "ready":
          return null;
        case "waiting_for_login":
        default:
          return {
            sections: [
              {
                text: t("battlenet.setupWaitingForLogin"),
                loading: true,
              },
              ...(detectedSection ? [detectedSection] : []),
              {
                lines: [t("battlenet.setupKeepMeLoggedIn")],
              },
            ],
          };
      }
    }

    switch (flow.status.state) {
      case "waiting_for_client":
        return {
          sections: [
            {
              text: t(getSetupKey(flow.platformId, "waitingForClient")),
              loading: true,
            },
          ],
        };
      case "failed":
        return {
          sections: [
            {
              title: t(getSetupKey(flow.platformId, "failed")),
              text: t(getSetupKey(flow.platformId, "failedMessage")),
            },
            ...(error
              ? [
                  {
                    lines: [error],
                    chips: [{ text: t("common.close"), tone: "red" as const }],
                  },
                ]
              : []),
          ],
        };
      case "ready":
        return null;
      case "waiting_for_login":
      default:
        return {
          sections: [
            {
              text: t(getSetupKey(flow.platformId, "waitingForLogin")),
              loading: true,
            },
            ...(detectedSection ? [detectedSection] : []),
          ],
        };
    }
  }

  function isForcedOpen(accountId: string): boolean {
    return Boolean(
      flow &&
      flow.platformId === getActiveTab() &&
      flow.status.setupId === accountId &&
      flow.status.state !== "ready",
    );
  }

  function isPendingSetupAccount(accountId: string): boolean {
    return Boolean(pendingSetupAccount && pendingSetupAccount.id === accountId);
  }

  return {
    get flow() {
      return flow;
    },
    get pendingSetupAccount() {
      return pendingSetupAccount;
    },
    clearTimer,
    stop,
    start,
    poll,
    cancel,
    cancelIfConflicting,
    getSetupExtensionContent,
    isForcedOpen,
    isPendingSetupAccount,
  };
}
