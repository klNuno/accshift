import type { PlatformAddFlowStatus } from "$lib/shared/platform";

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

