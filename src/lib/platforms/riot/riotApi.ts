import { invoke } from "@tauri-apps/api/core";
import type { RiotProfile, RiotStartupSnapshot } from "./types";

export async function getProfiles(): Promise<RiotProfile[]> {
  return invoke<RiotProfile[]>("get_riot_profiles");
}

export async function getCurrentProfile(): Promise<string> {
  return invoke<string>("get_current_riot_profile");
}

export async function getStartupSnapshot(): Promise<RiotStartupSnapshot> {
  return invoke<RiotStartupSnapshot>("get_riot_startup_snapshot");
}

export async function createProfile(): Promise<void> {
  await invoke("create_riot_profile");
}

export async function captureProfile(profileId: string): Promise<void> {
  await invoke("capture_riot_profile", { profileId });
}

export async function switchProfile(profileId: string): Promise<void> {
  await invoke("switch_riot_profile", { profileId });
}

export async function forgetProfile(profileId: string): Promise<void> {
  await invoke("forget_riot_profile", { profileId });
}
