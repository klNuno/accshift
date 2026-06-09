export type RiotSnapshotState = "setup_pending" | "awaiting_capture" | "capturing" | "ready";

export interface RiotProfile {
  id: string;
  label: string;
  account_name?: string;
  account_tag_line?: string;
  snapshot_state: RiotSnapshotState | string;
  last_captured_at?: number | null;
  last_used_at?: number | null;
}

export interface RiotStartupSnapshot {
  profiles: RiotProfile[];
  currentProfile: string;
}
