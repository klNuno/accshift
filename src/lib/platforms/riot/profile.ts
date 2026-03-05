import type { CachedPlatformProfile, PlatformProfileInfo } from "$lib/shared/platform";
import { getCachedRiotProfileMeta } from "./accountCache";

function getProfileDisplayName(profile: {
  label: string;
  account_name?: string;
  account_tag_line?: string;
}): string {
  const name = (profile.account_name ?? "").trim();
  const tagLine = (profile.account_tag_line ?? "").trim();
  if (!name) return profile.label;
  return tagLine ? `${name}#${tagLine}` : name;
}

function createAvatarSvg(accountId: string, displayName: string): string {
  const initials = displayName
    .split(/\s+/)
    .map((part) => part[0] ?? "")
    .join("")
    .slice(0, 2)
    .toUpperCase();
  const hue = Array.from(accountId).reduce((sum, char) => sum + char.charCodeAt(0), 0) % 360;
  const svg = `
    <svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128">
      <defs>
        <linearGradient id="bg" x1="0" x2="1" y1="0" y2="1">
          <stop offset="0%" stop-color="hsl(${hue} 74% 52%)" />
          <stop offset="100%" stop-color="hsl(${(hue + 28) % 360} 82% 38%)" />
        </linearGradient>
      </defs>
      <rect width="128" height="128" rx="26" fill="url(#bg)" />
      <text x="64" y="74" text-anchor="middle" font-family="Segoe UI, Arial, sans-serif" font-size="40" font-weight="700" fill="white">${initials}</text>
    </svg>
  `.replace(/\s+/g, " ").trim();
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

export function getRiotProfile(profileId: string): PlatformProfileInfo | null {
  const profile = getCachedRiotProfileMeta(profileId);
  if (!profile) return null;
  const displayName = getProfileDisplayName(profile);
  const avatarLoading = profile.snapshot_state === "setup_pending" || profile.snapshot_state === "capturing";
  return {
    avatarUrl: avatarLoading ? null : createAvatarSvg(profile.id, displayName),
    displayName,
    avatarLoading,
  };
}

export function getCachedRiotProfile(profileId: string): CachedPlatformProfile | null {
  const preview = getRiotProfile(profileId);
  if (!preview?.avatarUrl) return null;
  return {
    url: preview.avatarUrl,
    displayName: preview.displayName ?? undefined,
    expired: false,
  };
}
