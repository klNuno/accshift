export type RiotSnapshotState = "awaiting_capture" | "ready";

export interface RiotProfile {
  id: string;
  label: string;
  account_name: string;
  account_tag_line: string;
  account_puuid: string;
  snapshot_state: RiotSnapshotState | string;
  notes: string;
  last_captured_at?: number | null;
  last_used_at?: number | null;
}

export interface RiotStartupSnapshot {
  profiles: RiotProfile[];
  currentProfile: string;
}
