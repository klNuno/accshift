/**
 * The user's own platform descriptors, as the frontend sees them.
 *
 * A descriptor is a JSON file the user drops in their data folder; the Rust
 * engine runs it, and these calls are the app's side of that: read the folder,
 * judge a candidate file before installing it, install it, drop it.
 *
 * Every call re-reads the folder and returns the whole report, so the UI never
 * has to guess what the backend now believes.
 */
import { invoke } from "@tauri-apps/api/core";
import { registerUserPlatforms, type PlatformDescriptor } from "./registry";

/** What a dry run would do to one file, folder, registry value or process. */
export interface PlanStep {
  action: "read" | "capture" | "restore" | "delete" | "close" | "launch";
  kind: "file" | "directory" | "registryValue" | "process" | "executable";
  /** The live path, registry value or process name, fully resolved. */
  target: string;
  /** Where the data would come from or go, for a capture or a restore. */
  snapshot?: string;
  /** Why the step would be skipped, or anything else worth reading. */
  note?: string;
}

/** Everything an operation would do, having done none of it. */
export interface DryRunPlan {
  platformId: string;
  operation: string;
  accountId: string;
  /** Always false. A plan is never a report of work done. */
  applied: boolean;
  /** The folders the descriptor is allowed to touch. */
  roots: string[];
  steps: PlanStep[];
  warnings: string[];
}

/** A descriptor file judged without installing it. */
export interface DescriptorPreview {
  /** The file the user picked, as they picked it. */
  source: string;
  descriptor: PlatformDescriptor;
  /** The name it would take in the folder, always `<id>.json`. */
  fileName: string;
  /** A file of that name is already there, so this replaces rather than adds. */
  replaces: boolean;
  /** Why installing it would add no platform. Empty when it would. */
  blocked: string;
  plan: DryRunPlan | null;
  /** Why no plan could be built. Empty when there is one. */
  planProblem: string;
}

/** What the backend last read in the descriptor folder. */
export interface UserPlatformReport {
  dir: string;
  loaded: PlatformDescriptor[];
  skipped: { id: string; reason: string }[];
  rejected: { source: string; field: string; problem: string }[];
}

/**
 * Feeds a fresh report into the platform registry and hands it back.
 *
 * Every call that changes the folder goes through here, so the tab list and
 * the settings list can never disagree with the folder the engine reads.
 */
function adopt(report: UserPlatformReport): UserPlatformReport {
  registerUserPlatforms(report.loaded);
  return report;
}

/** Re-reads the folder. A file added, edited or deleted takes effect here. */
export async function reloadUserPlatforms(): Promise<UserPlatformReport> {
  return adopt(await invoke<UserPlatformReport>("reload_user_platforms"));
}

/**
 * Opens the file picker. Returns null when the user cancelled or the native
 * dialog failed, both of which mean the same thing here: leave everything
 * alone. Same reading as the launcher path picker in the settings.
 */
export async function selectDescriptorFile(): Promise<string | null> {
  try {
    return await invoke<string>("descriptor_select_file");
  } catch {
    return null;
  }
}

/** What the picked file would add, and what a switch on it would touch. */
export async function previewDescriptorFile(path: string): Promise<DescriptorPreview> {
  return await invoke<DescriptorPreview>("descriptor_preview_file", { path });
}

/** Copies the file into the folder and reloads. */
export async function installDescriptorFile(path: string): Promise<UserPlatformReport> {
  return adopt(await invoke<UserPlatformReport>("descriptor_install_file", { path }));
}

/** Deletes the file behind a user platform and reloads. */
export async function removeDescriptor(platformId: string): Promise<UserPlatformReport> {
  return adopt(await invoke<UserPlatformReport>("descriptor_remove", { platformId }));
}

/** Reveals the descriptor folder in the OS file manager. */
export async function openDescriptorsFolder(): Promise<void> {
  await invoke("open_descriptors_folder");
}

/** What switching this account would read, copy, write and close. */
export async function dryRunSwitch(platformId: string, accountId: string): Promise<DryRunPlan> {
  return await invoke<DryRunPlan>("platform_dry_run", { platformId, accountId });
}
