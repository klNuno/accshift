import type {
  PlatformAdapter,
  PlatformAddAccountResult,
  PlatformAddFlowStatus,
} from "$lib/shared/platform";

export interface RawAddFlowStatusPayload {
  state: string;
  accountId?: string;
  accountDisplayName?: string;
  errorMessage?: string;
}

export function toPlatformAddFlowStatus(
  setupId: string,
  payload: RawAddFlowStatusPayload,
): PlatformAddFlowStatus {
  return {
    setupId,
    state: payload.state,
    accountId: payload.accountId,
    accountDisplayName: payload.accountDisplayName,
    errorMessage: payload.errorMessage,
  };
}

interface PlatformAddFlowService {
  beginSetup(): Promise<PlatformAddFlowStatus>;
  getSetupStatus(setupId: string): Promise<PlatformAddFlowStatus>;
  cancelSetup(setupId: string): Promise<void>;
}

export function createPlatformAddFlowHandlers(
  service: PlatformAddFlowService,
): Pick<PlatformAdapter, "addAccount" | "pollAddFlow" | "cancelAddFlow"> {
  return {
    async addAccount(): Promise<PlatformAddAccountResult> {
      const setupStatus = await service.beginSetup();
      return { setupStatus };
    },

    async pollAddFlow(setupId: string): Promise<PlatformAddFlowStatus> {
      return service.getSetupStatus(setupId);
    },

    async cancelAddFlow(setupId: string): Promise<void> {
      await service.cancelSetup(setupId);
    },
  };
}

