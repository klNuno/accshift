import { invoke } from "@tauri-apps/api/core";
import type { PlatformAddFlowStatus } from "$lib/shared/platform";
import { logAppEvent, serializeLogValue } from "$lib/shared/appLogger";
import { toPlatformAddFlowStatus } from "$lib/platforms/addFlow";

interface SetupStatusPayload {
  setupId: string;
  state: string;
  accountId?: string;
  accountDisplayName?: string;
  errorMessage?: string;
}

export function createPlatformApi(platformId: string) {
  const logPrefix = `frontend.${platformId.replace("-", "_")}`;

  function getAccounts<T>(): Promise<T[]> {
    return invoke<T[]>("platform_get_accounts", { platformId });
  }

  function getCurrentAccount(): Promise<string> {
    return invoke<string>("platform_get_current_account", { platformId });
  }

  function getStartupSnapshot<T>(): Promise<T> {
    return invoke<T>("platform_get_startup_snapshot", { platformId });
  }

  async function switchAccount(
    accountId: string,
    params: Record<string, unknown> = {},
    logDetails?: Record<string, unknown>,
  ): Promise<void> {
    const details = logDetails ?? { accountId };
    void logAppEvent("info", `${logPrefix}.switch`, "Switch request started", details);
    try {
      await invoke("platform_switch_account", { platformId, accountId, params });
      void logAppEvent("info", `${logPrefix}.switch`, "Switch request completed", details);
    } catch (reason) {
      void logAppEvent("error", `${logPrefix}.switch`, "Switch request failed", {
        ...details,
        error: serializeLogValue(reason),
      });
      throw reason;
    }
  }

  async function beginSetup(params: Record<string, unknown> = {}): Promise<PlatformAddFlowStatus> {
    const payload = await invoke<SetupStatusPayload>("platform_begin_setup", {
      platformId,
      params,
    });
    return toPlatformAddFlowStatus(payload.setupId, payload);
  }

  async function getSetupStatus(setupId: string): Promise<PlatformAddFlowStatus> {
    const payload = await invoke<SetupStatusPayload>("platform_get_setup_status", {
      platformId,
      setupId,
    });
    return toPlatformAddFlowStatus(payload.setupId, payload);
  }

  async function cancelSetup(setupId: string): Promise<void> {
    await invoke("platform_cancel_setup", { platformId, setupId });
  }

  async function forgetAccount(accountId: string): Promise<void> {
    await invoke("platform_forget_account", { platformId, accountId });
  }

  function getPath(): Promise<string> {
    return invoke<string>("platform_get_path", { platformId });
  }

  function setPath(path: string): Promise<void> {
    return invoke("platform_set_path", { platformId, path });
  }

  function selectPath(): Promise<string> {
    return invoke<string>("platform_select_path", { platformId });
  }

  return {
    getAccounts,
    getCurrentAccount,
    getStartupSnapshot,
    switchAccount,
    beginSetup,
    getSetupStatus,
    cancelSetup,
    forgetAccount,
    getPath,
    setPath,
    selectPath,
  };
}
